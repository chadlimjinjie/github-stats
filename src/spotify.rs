//! Spotify OAuth (authorization code) and the now-playing / last-played track.

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde_json::Value;

use crate::config::Config;

const SCOPES: &str = "user-read-currently-playing user-read-recently-played";

#[derive(Clone, Debug)]
pub struct Track {
    pub name: String,
    pub artists: String,
    pub playing: bool,
    /// Album art as a `data:` URI; camo strips external images inside SVGs.
    pub art: Option<String>,
}

pub fn authorize_url(cfg: &Config, state: &str) -> String {
    reqwest::Url::parse_with_params(
        "https://accounts.spotify.com/authorize",
        [
            ("client_id", cfg.spotify_client_id.as_str()),
            ("response_type", "code"),
            ("redirect_uri", cfg.spotify_redirect_uri.as_str()),
            ("scope", SCOPES),
            ("state", state),
        ],
    )
    .map(String::from)
    .unwrap_or_default()
}

/// POST to the token endpoint; returns the JSON body or Spotify's error.
async fn token(http: &reqwest::Client, cfg: &Config, form: &[(&str, &str)]) -> Result<Value, String> {
    let resp = http
        .post("https://accounts.spotify.com/api/token")
        .basic_auth(&cfg.spotify_client_id, Some(&cfg.spotify_client_secret))
        .form(form)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = resp.status();
    let body: Value = resp.json().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("spotify token {status}: {}", body["error"]));
    }
    Ok(body)
}

/// Exchange the callback `code` for a refresh token.
pub async fn exchange_code(http: &reqwest::Client, cfg: &Config, code: &str) -> Result<String, String> {
    let body = token(
        http,
        cfg,
        &[("grant_type", "authorization_code"), ("code", code), ("redirect_uri", &cfg.spotify_redirect_uri)],
    )
    .await?;
    body["refresh_token"].as_str().map(String::from).ok_or_else(|| "no refresh_token".into())
}

/// The current track, else the last played one. Also returns a rotated refresh token if Spotify issued one.
pub async fn now_playing(
    http: &reqwest::Client,
    cfg: &Config,
    refresh_token: &str,
) -> Result<(Option<Track>, Option<String>), String> {
    let tok = token(http, cfg, &[("grant_type", "refresh_token"), ("refresh_token", refresh_token)]).await?;
    let access = tok["access_token"].as_str().ok_or("no access_token")?;
    let rotated = tok["refresh_token"].as_str().map(String::from);

    let get = |url: &'static str| http.get(url).bearer_auth(access).send();
    let current = get("https://api.spotify.com/v1/me/player/currently-playing").await.map_err(|e| e.to_string())?;
    // 204 = nothing playing
    let (item, playing) = if current.status() == reqwest::StatusCode::OK {
        let v: Value = current.json().await.map_err(|e| e.to_string())?;
        (v["item"].clone(), v["is_playing"].as_bool().unwrap_or(false))
    } else {
        (Value::Null, false)
    };
    let item = if item.is_object() {
        item
    } else {
        let v: Value = get("https://api.spotify.com/v1/me/player/recently-played?limit=1")
            .await
            .map_err(|e| e.to_string())?
            .json()
            .await
            .map_err(|e| e.to_string())?;
        v["items"][0]["track"].clone()
    };
    if !item.is_object() {
        return Ok((None, rotated));
    }

    let artists = item["artists"]
        .as_array()
        .map(|a| a.iter().filter_map(|x| x["name"].as_str()).collect::<Vec<_>>().join(", "))
        .unwrap_or_default();
    // Images are largest first; the middle one (~300px) is plenty for a 100px tile.
    let images = item["album"]["images"].as_array().cloned().unwrap_or_default();
    let art_url = images.get(1).or(images.first()).and_then(|i| i["url"].as_str());
    let art = match art_url {
        Some(url) => fetch_data_uri(http, url).await.ok(),
        None => None,
    };
    Ok((
        Some(Track { name: item["name"].as_str().unwrap_or("").to_string(), artists, playing, art }),
        rotated,
    ))
}

async fn fetch_data_uri(http: &reqwest::Client, url: &str) -> Result<String, String> {
    let resp = http.get(url).send().await.map_err(|e| e.to_string())?;
    let mime = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .filter(|m| m.starts_with("image/"))
        .unwrap_or("image/jpeg")
        .to_string();
    let bytes = resp.bytes().await.map_err(|e| e.to_string())?;
    Ok(format!("data:{mime};base64,{}", STANDARD.encode(bytes)))
}
