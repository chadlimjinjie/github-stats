//! HTTP handlers. JSON bodies are camelCase; errors are `{"error": "..."}`.
//! The public `/stats/u/{slug}.svg` always answers with an SVG, even for errors,
//! so a README never shows a broken image.

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::auth::AuthUser;
use crate::config::Config;
use crate::crypto::Sealer;
use crate::db::{self, Card, Item, Layout, Pool, Profile, Theme};
use crate::github::{self, GithubData};
use crate::render::{self, Data};
use crate::spotify::{self, Track};

// ponytail: per-process cache; move to Redis if this runs as more than one instance.
type Ttl<T> = Arc<Mutex<HashMap<String, (Instant, Result<T, String>)>>>;

const GITHUB_TTL: Duration = Duration::from_secs(4 * 3600);
const SPOTIFY_TTL: Duration = Duration::from_secs(30);
const ERROR_TTL: Duration = Duration::from_secs(300);
const STATE_TTL_SECS: u64 = 600;
const MAX_CUSTOM: usize = 5;
const MAX_TITLE: usize = 38;
const MAX_TEXT: usize = 280;

#[derive(Clone)]
pub struct AppState {
    pub pool: Pool,
    pub http: reqwest::Client,
    pub cfg: Arc<Config>,
    pub sealer: Sealer,
    pub github: Ttl<GithubData>,
    pub spotify: Ttl<Option<Track>>,
}

pub async fn root() -> Json<Value> {
    Json(json!({ "ok": true, "service": "midtone-stats", "version": env!("CARGO_PKG_VERSION") }))
}

fn err(code: StatusCode, msg: &str) -> Response {
    (code, Json(json!({ "error": msg }))).into_response()
}

fn internal(ctx: &str, e: impl std::fmt::Display) -> Response {
    tracing::error!("{ctx}: {e}");
    err(StatusCode::INTERNAL_SERVER_ERROR, "Internal error")
}

async fn cached<T: Clone>(cache: &Ttl<T>, key: &str, ttl: Duration, fetch: impl Future<Output = Result<T, String>>) -> Result<T, String> {
    if let Some((at, v)) = cache.lock().unwrap().get(key) {
        if at.elapsed() < if v.is_ok() { ttl } else { ttl.min(ERROR_TTL) } {
            return v.clone();
        }
    }
    let v = fetch.await;
    cache.lock().unwrap().insert(key.to_string(), (Instant::now(), v.clone()));
    v
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

// ---- profile -------------------------------------------------------------

pub async fn get_me(State(st): State<AppState>, AuthUser(uid): AuthUser) -> Response {
    let profile = match db::profile_by_user(&st.pool, &uid).await {
        Ok(p) => p,
        Err(e) => return internal("profile_by_user", e),
    };
    let linked = match db::spotify_token(&st.pool, &uid).await {
        Ok(t) => t.is_some(),
        Err(e) => return internal("spotify_token", e),
    };
    Json(json!({ "profile": profile, "spotifyLinked": linked })).into_response()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PutBody {
    slug: String,
    #[serde(default)]
    github_login: String,
    layout: Layout,
    theme: Theme,
}

fn validate(b: &mut PutBody) -> Result<(), &'static str> {
    b.slug = b.slug.trim().to_lowercase();
    b.github_login = b.github_login.trim().to_string();
    let slug_ok = (3..=32).contains(&b.slug.len()) && b.slug.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-');
    if !slug_ok {
        return Err("Slug must be 3-32 characters: a-z, 0-9 or -");
    }
    let login_ok = b.github_login.len() <= 39 && b.github_login.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-');
    if !login_ok {
        return Err("Invalid GitHub username");
    }
    if !matches!(b.layout.columns, 1 | 2) {
        return Err("Columns must be 1 or 2");
    }
    let mut seen = Vec::new();
    b.layout.cards.retain(|c| match c {
        Item::Card(c) => !seen.contains(c) && {
            seen.push(*c);
            true
        },
        Item::Custom { .. } => true,
    });
    let mut customs = 0;
    for c in &mut b.layout.cards {
        if let Item::Custom { title, text } = c {
            customs += 1;
            *title = title.trim().to_string();
            *text = text.trim().to_string();
            if title.is_empty() || title.chars().count() > MAX_TITLE {
                return Err("Custom card title must be 1-38 characters");
            }
            if text.chars().count() > MAX_TEXT {
                return Err("Custom card text must be at most 280 characters");
            }
        }
    }
    if customs > MAX_CUSTOM {
        return Err("At most 5 custom cards");
    }
    Ok(())
}

pub async fn put_me(State(st): State<AppState>, AuthUser(uid): AuthUser, body: Bytes) -> Response {
    let Ok(mut b) = serde_json::from_slice::<PutBody>(&body) else {
        return err(StatusCode::BAD_REQUEST, "Invalid request body");
    };
    if let Err(msg) = validate(&mut b) {
        return err(StatusCode::BAD_REQUEST, msg);
    }
    let p = Profile { user_id: uid, slug: b.slug, github_login: b.github_login, layout: b.layout, theme: b.theme };
    match db::upsert_profile(&st.pool, &p).await {
        Ok(p) => Json(p).into_response(),
        Err(e) if e.as_database_error().and_then(|d| d.code()).as_deref() == Some("23505") => {
            err(StatusCode::CONFLICT, "That slug is taken")
        }
        Err(e) => internal("upsert_profile", e),
    }
}

// ---- spotify -------------------------------------------------------------

pub async fn spotify_link(State(st): State<AppState>, AuthUser(uid): AuthUser) -> Response {
    if st.cfg.spotify_client_id.is_empty() {
        return err(StatusCode::SERVICE_UNAVAILABLE, "Spotify is not configured");
    }
    match st.sealer.seal(&format!("{uid}|{}", now() + STATE_TTL_SECS)) {
        Ok(state) => Json(json!({ "url": spotify::authorize_url(&st.cfg, &state) })).into_response(),
        Err(e) => internal("seal state", e),
    }
}

/// The user id from a sealed `state`, if it is ours and not expired.
fn open_state(sealer: &Sealer, state: &str, now: u64) -> Option<String> {
    let plain = sealer.open(state).ok()?;
    let (uid, exp) = plain.rsplit_once('|')?;
    (exp.parse::<u64>().ok()? >= now).then(|| uid.to_string())
}

#[derive(Deserialize)]
pub struct Callback {
    code: Option<String>,
    state: Option<String>,
}

pub async fn spotify_callback(State(st): State<AppState>, Query(q): Query<Callback>) -> Response {
    let back = |status: &str| Redirect::to(&format!("{}/?spotify={status}", st.cfg.web_stats_url)).into_response();
    // No code means the user declined on Spotify's page.
    let (Some(code), Some(uid)) = (q.code, q.state.and_then(|s| open_state(&st.sealer, &s, now()))) else {
        return back("error");
    };
    let sealed = match spotify::exchange_code(&st.http, &st.cfg, &code).await.and_then(|t| st.sealer.seal(&t)) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("spotify callback: {e}");
            return back("error");
        }
    };
    if let Err(e) = db::set_spotify_token(&st.pool, &uid, &sealed).await {
        tracing::error!("set_spotify_token: {e}");
        return back("error");
    }
    st.spotify.lock().unwrap().remove(&uid);
    back("linked")
}

pub async fn spotify_unlink(State(st): State<AppState>, AuthUser(uid): AuthUser) -> Response {
    if let Err(e) = db::delete_spotify(&st.pool, &uid).await {
        return internal("delete_spotify", e);
    }
    st.spotify.lock().unwrap().remove(&uid);
    StatusCode::NO_CONTENT.into_response()
}

async fn spotify_track(st: &AppState, uid: &str) -> Result<Option<Track>, String> {
    let sealed = db::spotify_token(&st.pool, uid).await.map_err(|e| e.to_string())?.ok_or("Spotify not linked")?;
    let refresh = st.sealer.open(&sealed)?;
    let (track, rotated) = spotify::now_playing(&st.http, &st.cfg, &refresh).await?;
    if let Some(sealed) = rotated.and_then(|t| st.sealer.seal(&t).ok()) {
        let _ = db::set_spotify_token(&st.pool, uid, &sealed).await;
    }
    Ok(track)
}

// ---- public svg ----------------------------------------------------------

fn svg(status: StatusCode, body: String) -> Response {
    (
        status,
        [
            (header::CONTENT_TYPE, "image/svg+xml; charset=utf-8".to_string()),
            // GitHub's Camo proxy obeys this; no-cache makes README edits show on next load.
            (header::CACHE_CONTROL, "no-cache, max-age=0".to_string()),
            // Opened directly in a browser, the SVG can never run script.
            (header::CONTENT_SECURITY_POLICY, "default-src 'none'; img-src data:; style-src 'unsafe-inline'".to_string()),
        ],
        body,
    )
        .into_response()
}

pub async fn card(State(st): State<AppState>, Path(file): Path<String>) -> Response {
    let slug = file.strip_suffix(".svg").unwrap_or(&file);
    let profile = match db::profile_by_slug(&st.pool, slug).await {
        Ok(Some(p)) => p,
        Ok(None) => {
            let body = render::compose(&[Data::Error(Card::Github, "no such profile")], Theme::Light, 1);
            return svg(StatusCode::NOT_FOUND, body);
        }
        Err(e) => {
            tracing::error!("profile_by_slug: {e}");
            let body = render::compose(&[Data::Error(Card::Github, "try again later")], Theme::Light, 1);
            return svg(StatusCode::SERVICE_UNAVAILABLE, body);
        }
    };
    let cards = &profile.layout.cards;

    let gh = if cards.iter().any(|c| matches!(c, Item::Card(c) if *c != Card::Spotify)) {
        if profile.github_login.is_empty() {
            Err("no GitHub username set".to_string())
        } else {
            let key = profile.github_login.to_lowercase();
            cached(&st.github, &key, GITHUB_TTL, github::fetch(&st.http, &st.cfg.github_token, &profile.github_login)).await
        }
    } else {
        Err(String::new())
    };
    let sp = if cards.contains(&Item::Card(Card::Spotify)) {
        cached(&st.spotify, &profile.user_id, SPOTIFY_TTL, spotify_track(&st, &profile.user_id)).await
    } else {
        Err(String::new())
    };

    let data: Vec<Data> = cards
        .iter()
        .map(|item| match item {
            Item::Custom { title, text } => Data::Custom(title, text),
            Item::Card(c) => builtin(c, &gh, &sp),
        })
        .collect();
    svg(StatusCode::OK, render::compose(&data, profile.theme, profile.layout.columns))
}

fn builtin<'a>(c: &Card, gh: &'a Result<GithubData, String>, sp: &'a Result<Option<Track>, String>) -> Data<'a> {
    match (c, gh, sp) {
        (Card::Spotify, _, Ok(t)) => Data::Spotify(t.as_ref()),
        (Card::Spotify, _, Err(e)) => Data::Error(*c, e),
        (_, Err(e), _) => Data::Error(*c, e),
        (Card::Github, Ok(d), _) => Data::Github(d),
        (Card::Languages, Ok(d), _) => Data::Languages(d),
        (Card::Streak, Ok(d), _) => Data::Streak(d),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn body(slug: &str, login: &str, cards: Vec<Item>, columns: u8) -> PutBody {
        PutBody { slug: slug.into(), github_login: login.into(), layout: Layout { cards, columns }, theme: Theme::Light }
    }

    fn custom(title: &str, text: &str) -> Item {
        Item::Custom { title: title.into(), text: text.into() }
    }

    #[test]
    fn validates_profile() {
        let (gh, sp) = (Item::Card(Card::Github), Item::Card(Card::Spotify));
        let mut b = body(" Chad-1 ", "octo-cat", vec![gh.clone(), custom(" Hi ", "x"), sp.clone(), gh.clone(), custom("Hi", "y")], 2);
        assert!(validate(&mut b).is_ok());
        assert_eq!(b.slug, "chad-1");
        assert_eq!(b.layout.cards, vec![gh.clone(), custom("Hi", "x"), sp, custom("Hi", "y")]);
        assert!(validate(&mut body("abc", "", vec![custom("  ", "x")], 1)).is_err());
        assert!(validate(&mut body("abc", "", vec![custom(&"t".repeat(39), "")], 1)).is_err());
        assert!(validate(&mut body("abc", "", vec![custom("t", &"é".repeat(280))], 1)).is_ok());
        assert!(validate(&mut body("abc", "", vec![custom("t", &"x".repeat(281))], 1)).is_err());
        assert!(validate(&mut body("abc", "", vec![custom("t", ""); 6], 1)).is_err());
        assert!(validate(&mut body("ab", "", vec![], 1)).is_err());
        assert!(validate(&mut body("a_bc", "", vec![], 1)).is_err());
        assert!(validate(&mut body("abc", "bad/login", vec![], 1)).is_err());
        assert!(validate(&mut body("abc", "", vec![], 3)).is_err());
    }

    #[test]
    fn state_round_trip_and_expiry() {
        let s = Sealer::new(KEY);
        let state = s.seal("user|with|pipes|1000").unwrap();
        assert_eq!(open_state(&s, &state, 999).as_deref(), Some("user|with|pipes"));
        assert_eq!(open_state(&s, &state, 1001), None);
        assert_eq!(open_state(&s, "garbage", 0), None);
        assert_eq!(open_state(&Sealer::new(&KEY.replace('0', "1")), &state, 0), None);
    }
}
