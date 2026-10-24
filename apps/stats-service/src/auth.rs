//! Verifies the caller's JWT against auth-service's JWKS for this tenant
//! (`<AUTH_SERVICE_URL>/auth/t/<AUTH_TENANT>/jwks`). Based on the bot services'
//! `auth/mod.rs`, plus the `admin` role from the `scope` claim.
//!
//! Handlers take `AuthUser` (401 without a valid bearer token) or `AdminUser`
//! (also 403 unless the token's scope includes `admin`).

use std::sync::{Arc, OnceLock, RwLock};
use std::time::Duration;

use axum::extract::FromRequestParts;
use axum::http::header::AUTHORIZATION;
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use jsonwebtoken::jwk::JwkSet;
use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use serde::Deserialize;
use serde_json::{json, Value};

struct AuthState {
    jwks_url: String,
    keys: RwLock<Option<JwkSet>>,
}

static STATE: OnceLock<Arc<AuthState>> = OnceLock::new();

pub struct User {
    pub id: String,
    pub is_admin: bool,
}

#[derive(Deserialize)]
struct Claims {
    // The user id is `sub`; tokens from the previous auth server carried it in `id`.
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    sub: Option<String>,
    // Space-separated; auth-service grants role scopes (e.g. "admin") here.
    // Kept as a raw value so a non-string scope doesn't reject the token.
    #[serde(default)]
    scope: Option<Value>,
}

/// Point verification at the tenant's JWKS and start the refresh loop.
/// A failed fetch is only logged; verification uses the next good fetch.
pub fn init(auth_service_url: &str, tenant: &str) {
    let jwks_url = format!("{}/auth/t/{tenant}/jwks", auth_service_url.trim_end_matches('/'));
    let state = Arc::new(AuthState {
        jwks_url,
        keys: RwLock::new(None),
    });
    let _ = STATE.set(state.clone());

    tokio::spawn(async move {
        loop {
            match fetch_jwks(&state.jwks_url).await {
                Ok(set) => {
                    if let Ok(mut guard) = state.keys.write() {
                        *guard = Some(set);
                    }
                }
                Err(e) => tracing::warn!("stats-service: failed to refresh JWKS: {e}"),
            }
            // Retry quickly until the first fetch succeeds (auth-service may start after us).
            let loaded = state.keys.read().is_ok_and(|k| k.is_some());
            tokio::time::sleep(Duration::from_secs(if loaded { 300 } else { 5 })).await;
        }
    });
}

async fn fetch_jwks(url: &str) -> Result<JwkSet, String> {
    let resp = reqwest::get(url).await.map_err(|e| e.to_string())?;
    resp.json::<JwkSet>().await.map_err(|e| e.to_string())
}

/// Verify an RS256 bearer token against the cached JWKS, returning the user.
pub fn verify_token(token: &str) -> Result<User, String> {
    let state = STATE.get().ok_or("auth not initialized")?;

    let header = decode_header(token).map_err(|e| format!("bad header: {e}"))?;
    let kid = header.kid.ok_or("token missing kid")?;

    let guard = state.keys.read().map_err(|_| "jwks lock poisoned")?;
    let set = guard.as_ref().ok_or("jwks not loaded yet")?;
    let jwk = set.find(&kid).ok_or("no matching jwk for kid")?;

    let key = DecodingKey::from_jwk(jwk).map_err(|e| format!("decode key: {e}"))?;
    let mut validation = Validation::new(Algorithm::RS256);
    // aud is the issuer URL, which differs from AUTH_SERVICE_URL behind the gateway;
    // exp/nbf are still enforced by the lib.
    validation.validate_aud = false;

    let data = decode::<Claims>(token, &key, &validation).map_err(|e| format!("verify: {e}"))?;
    user_from_claims(data.claims)
}

fn user_from_claims(c: Claims) -> Result<User, String> {
    let id = c
        .id
        .filter(|s| !s.is_empty())
        .or(c.sub)
        .filter(|s| !s.is_empty())
        .ok_or("missing id/sub claim")?;
    let is_admin = c
        .scope
        .as_ref()
        .and_then(Value::as_str)
        .is_some_and(|s| s.split_whitespace().any(|w| w == "admin"));
    Ok(User { id, is_admin })
}

#[allow(clippy::result_large_err)]
fn bearer_user(parts: &Parts) -> Result<User, Response> {
    let header = parts
        .headers
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let token = header.strip_prefix("Bearer ").ok_or_else(unauthorized)?;
    verify_token(token).map_err(|_| unauthorized())
}

/// axum extractor: requires a valid `Authorization: Bearer <jwt>`; yields the user id.
pub struct AuthUser(pub String);

impl<S: Send + Sync> FromRequestParts<S> for AuthUser {
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        bearer_user(parts).map(|u| AuthUser(u.id))
    }
}

/// axum extractor: like `AuthUser`, and also requires the `admin` scope (403).
pub struct AdminUser;

impl<S: Send + Sync> FromRequestParts<S> for AdminUser {
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        if bearer_user(parts)?.is_admin {
            Ok(AdminUser)
        } else {
            Err((
                StatusCode::FORBIDDEN,
                Json(json!({ "error": "Admin access required" })),
            )
                .into_response())
        }
    }
}

fn unauthorized() -> Response {
    (StatusCode::UNAUTHORIZED, Json(json!({ "error": "Unauthorized" }))).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(claims: Value) -> Result<User, String> {
        user_from_claims(serde_json::from_value(claims).unwrap())
    }

    #[test]
    fn maps_admin_scope() {
        for (scope, admin) in [
            (json!("openid admin organizations"), true),
            (json!("openid administrator"), false),
            (json!(["admin"]), false), // only the string form counts
            (Value::Null, false),
        ] {
            let u = user(json!({ "sub": "u1", "scope": scope })).unwrap();
            assert_eq!(u.id, "u1");
            assert_eq!(u.is_admin, admin, "scope {scope}");
        }
    }

    #[test]
    fn id_falls_back_to_sub() {
        assert_eq!(user(json!({ "id": "", "sub": "s" })).unwrap().id, "s");
        assert_eq!(user(json!({ "id": "i", "sub": "s" })).unwrap().id, "i");
        assert!(user(json!({})).is_err());
    }
}
