-- Tables owned by stats-service. Applied on every startup (see mod.rs), so each
-- statement must be safe to re-run: CREATE ... IF NOT EXISTS, ADD COLUMN IF NOT EXISTS.

-- One public stats page per midtone user, served at /stats/u/<slug>.svg.
-- "layout" is {"cards": ["github", "languages", "streak", "spotify", {"title": "..", "text": ".."}], "columns": 1|2},
-- in display order; objects are user-written custom cards, or {"title": "..", "images": ["data:image/jpeg;base64,.."]} galleries.
CREATE TABLE IF NOT EXISTS stats_profile (
    "user_id"      text PRIMARY KEY,
    "slug"         text NOT NULL UNIQUE,
    "github_login" text NOT NULL DEFAULT '',
    "layout"       jsonb NOT NULL,
    "theme"        text NOT NULL DEFAULT 'light',
    "updated_at"   timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Linked Spotify account; the refresh token is AES-GCM sealed (crypto.rs).
CREATE TABLE IF NOT EXISTS spotify_link (
    "user_id"           text PRIMARY KEY,
    "refresh_token_enc" text NOT NULL,
    "updated_at"        timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP
);
