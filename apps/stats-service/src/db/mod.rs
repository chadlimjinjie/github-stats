//! Postgres access via sqlx. Tables (`stats_profile`, `spotify_link`) are created
//! on startup from `schema.sql`. Queries use `.persistent(false)`, as in api-service,
//! so they are safe behind the Supabase transaction pooler.

use std::str::FromStr;

use serde::{Deserialize, Serialize};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions, PgRow};
use sqlx::types::Json;
use sqlx::{PgPool, Row};

pub type Pool = PgPool;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Card {
    Github,
    Languages,
    Streak,
    Spotify,
}

/// A layout entry: a built-in card name, or a user's own `{"title", "text"}` card.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Item {
    Card(Card),
    Custom { title: String, text: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    Light,
    Dark,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Layout {
    pub cards: Vec<Item>,
    pub columns: u8,
    /// Per card: spans both columns. Parallel to `cards`; missing in rows saved before it existed.
    #[serde(default)]
    pub wide: Vec<bool>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    #[serde(skip)]
    pub user_id: String,
    pub slug: String,
    pub github_login: String,
    pub layout: Layout,
    pub theme: Theme,
}

const COLS: &str = r#""user_id", "slug", "github_login", "layout", "theme""#;

pub async fn connect(database_url: &str) -> Result<Pool, String> {
    let opts = PgConnectOptions::from_str(database_url)
        .map_err(|e| e.to_string())?
        .statement_cache_capacity(0);
    PgPoolOptions::new()
        .max_connections(5)
        .connect_with(opts)
        .await
        .map_err(|e| e.to_string())
}

pub async fn apply_schema(pool: &Pool) -> Result<(), String> {
    sqlx::raw_sql(include_str!("schema.sql"))
        .execute(pool)
        .await
        .map(|_| ())
        .map_err(|e| format!("apply schema: {e}"))
}

fn profile_from_row(row: &PgRow) -> Result<Profile, sqlx::Error> {
    let theme: String = row.try_get("theme")?;
    Ok(Profile {
        user_id: row.try_get("user_id")?,
        slug: row.try_get("slug")?,
        github_login: row.try_get("github_login")?,
        layout: row.try_get::<Json<Layout>, _>("layout")?.0,
        theme: if theme == "dark" { Theme::Dark } else { Theme::Light },
    })
}

/// `column` is a trusted constant ("user_id" or "slug").
async fn profile_by(pool: &Pool, column: &str, value: &str) -> Result<Option<Profile>, sqlx::Error> {
    let sql = format!(r#"SELECT {COLS} FROM stats_profile WHERE "{column}" = $1"#);
    let row = sqlx::query(&sql).persistent(false).bind(value).fetch_optional(pool).await?;
    row.as_ref().map(profile_from_row).transpose()
}

pub async fn profile_by_user(pool: &Pool, user_id: &str) -> Result<Option<Profile>, sqlx::Error> {
    profile_by(pool, "user_id", user_id).await
}

pub async fn profile_by_slug(pool: &Pool, slug: &str) -> Result<Option<Profile>, sqlx::Error> {
    profile_by(pool, "slug", slug).await
}

/// Fails with a unique violation (23505) when the slug belongs to another user.
pub async fn upsert_profile(pool: &Pool, p: &Profile) -> Result<Profile, sqlx::Error> {
    let sql = format!(
        r#"INSERT INTO stats_profile ({COLS}, "updated_at") VALUES ($1, $2, $3, $4, $5, CURRENT_TIMESTAMP)
           ON CONFLICT ("user_id") DO UPDATE SET "slug" = $2, "github_login" = $3, "layout" = $4,
               "theme" = $5, "updated_at" = CURRENT_TIMESTAMP
           RETURNING {COLS}"#
    );
    let theme = match p.theme {
        Theme::Light => "light",
        Theme::Dark => "dark",
    };
    let row = sqlx::query(&sql)
        .persistent(false)
        .bind(&p.user_id)
        .bind(&p.slug)
        .bind(&p.github_login)
        .bind(Json(&p.layout))
        .bind(theme)
        .fetch_one(pool)
        .await?;
    profile_from_row(&row)
}

pub async fn spotify_token(pool: &Pool, user_id: &str) -> Result<Option<String>, sqlx::Error> {
    let row = sqlx::query(r#"SELECT "refresh_token_enc" FROM spotify_link WHERE "user_id" = $1"#)
        .persistent(false)
        .bind(user_id)
        .fetch_optional(pool)
        .await?;
    row.map(|r| r.try_get("refresh_token_enc")).transpose()
}

pub async fn set_spotify_token(pool: &Pool, user_id: &str, sealed: &str) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"INSERT INTO spotify_link ("user_id", "refresh_token_enc", "updated_at") VALUES ($1, $2, CURRENT_TIMESTAMP)
           ON CONFLICT ("user_id") DO UPDATE SET "refresh_token_enc" = $2, "updated_at" = CURRENT_TIMESTAMP"#,
    )
    .persistent(false)
    .bind(user_id)
    .bind(sealed)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn delete_spotify(pool: &Pool, user_id: &str) -> Result<(), sqlx::Error> {
    sqlx::query(r#"DELETE FROM spotify_link WHERE "user_id" = $1"#)
        .persistent(false)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(())
}
