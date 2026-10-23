//! stats-service: composite README cards behind the gateway at `/stats/*`.
//! - `/stats/me`: JWT-authed read/save of the caller's card page (slug, GitHub username, layout, theme).
//! - `/stats/spotify/*`: link/unlink a Spotify account (OAuth code flow).
//! - `/stats/u/{slug}.svg`: public SVG of the arranged cards, for `![](...)` in a README.
//! - Creates its tables (`stats_profile`, `spotify_link`) on startup.

// Copy of api-service `auth.rs`; `AdminUser` is unused here.
#[allow(dead_code)]
mod auth;
mod config;
mod crypto;
mod db;
mod github;
mod http;
mod render;
mod spotify;

use std::sync::Arc;
use std::time::Duration;

use axum::routing::{get, post};
use axum::Router;
use tracing_subscriber::EnvFilter;

use crate::http::AppState;

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    let config = config::Config::from_env().unwrap_or_else(|e| panic!("config: {e}"));
    auth::init(&config.auth_service_url, &config.auth_tenant);

    let pool = db::connect(&config.database_url)
        .await
        .unwrap_or_else(|e| panic!("db connect: {e}"));
    db::apply_schema(&pool).await.unwrap_or_else(|e| panic!("{e}"));

    let state = AppState {
        pool,
        // GitHub's image proxy gives up after a few seconds, so don't wait long upstream.
        http: reqwest::Client::builder()
            .timeout(Duration::from_secs(8))
            .build()
            .unwrap_or_else(|e| panic!("http client: {e}")),
        sealer: crypto::Sealer::new(&config.encryption_key),
        github: Default::default(),
        spotify: Default::default(),
        cfg: Arc::new(config),
    };
    let addr = format!("[::]:{}", state.cfg.port);

    let app = Router::new()
        .route("/stats", get(http::root))
        .route("/stats/me", get(http::get_me).put(http::put_me))
        .route("/stats/spotify/link", post(http::spotify_link))
        .route("/stats/spotify/callback", get(http::spotify_callback))
        .route("/stats/spotify", axum::routing::delete(http::spotify_unlink))
        .route("/stats/u/{file}", get(http::card))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|e| panic!("failed to bind {addr}: {e}"));
    tracing::info!("stats-service listening on {addr}");
    axum::serve(listener, app).await.unwrap_or_else(|e| panic!("serve: {e}"));
}
