//! Every environment variable the service reads, loaded once at startup.

pub struct Config {
    pub database_url: String,
    pub auth_service_url: String,
    pub auth_tenant: String,
    pub port: String,
    /// Server PAT for GitHub GraphQL; empty means GitHub cards render "unavailable".
    pub github_token: String,
    pub spotify_client_id: String,
    pub spotify_client_secret: String,
    pub spotify_redirect_uri: String,
    pub encryption_key: String,
    /// Where the Spotify callback sends the browser back to.
    pub web_stats_url: String,
}

impl Config {
    pub fn from_env() -> Result<Self, String> {
        Ok(Self {
            database_url: std::env::var("DATABASE_URL").map_err(|_| "DATABASE_URL not set")?,
            auth_service_url: env_or("AUTH_SERVICE_URL", "http://localhost:4112"),
            auth_tenant: env_or("AUTH_TENANT", "midtone"),
            port: env_or("PORT", "4106"),
            github_token: env_or("GITHUB_TOKEN", ""),
            spotify_client_id: env_or("SPOTIFY_CLIENT_ID", ""),
            spotify_client_secret: env_or("SPOTIFY_CLIENT_SECRET", ""),
            spotify_redirect_uri: env_or("SPOTIFY_REDIRECT_URI", "http://127.0.0.1:9080/stats/spotify/callback"),
            encryption_key: env_or("STATS_ENCRYPTION_KEY", ""),
            web_stats_url: env_or("WEB_STATS_URL", "http://localhost:5177").trim_end_matches('/').to_string(),
        })
    }
}

fn env_or(key: &str, fallback: &str) -> String {
    std::env::var(key).ok().filter(|v| !v.is_empty()).unwrap_or_else(|| fallback.to_string())
}
