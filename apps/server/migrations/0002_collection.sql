-- Collection (milestone 04): the identity root secret, daily identity keys, tracked links and
-- pixels, and the timezone epoch that day keys are scoped by (decision 0001).

-- 32 random bytes created once. Cache-token and persistent-profile keys derive from it.
CREATE TABLE installation_secret (
    id         INTEGER PRIMARY KEY CHECK (id = 1),
    root       BLOB NOT NULL CHECK (length(root) = 32),
    created_at INTEGER NOT NULL
) STRICT;

-- Starts at 1 and increments with each site timezone change (0001 Timezone change).
ALTER TABLE sites ADD COLUMN timezone_epoch INTEGER NOT NULL DEFAULT 1;

-- One random key per source, timezone epoch and local day. `site_id` is a site, link or pixel
-- ID; links and pixels use UTC days. Destruction sets `key` to NULL at `destroy_after`
-- (end of the day plus 25 hours) and keeps the row as a tombstone, so no key is ever created
-- for that day again.
CREATE TABLE identity_day_keys (
    site_id        TEXT NOT NULL,
    timezone_epoch INTEGER NOT NULL,
    local_day      INTEGER NOT NULL,
    key            BLOB CHECK (key IS NULL OR length(key) = 32),
    created_at     INTEGER NOT NULL,
    destroy_after  INTEGER NOT NULL,
    destroyed_at   INTEGER,
    PRIMARY KEY (site_id, timezone_epoch, local_day)
) STRICT, WITHOUT ROWID;

CREATE INDEX identity_day_keys_live ON identity_day_keys (destroy_after) WHERE key IS NOT NULL;

-- `GET /q/{slug}` targets. `lost_hits` counts hits that could not be stored (0005).
CREATE TABLE links (
    id           TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    slug         TEXT NOT NULL UNIQUE,
    url          TEXT NOT NULL,
    lost_hits    INTEGER NOT NULL DEFAULT 0,
    created_at   INTEGER NOT NULL,
    deleted_at   INTEGER
) STRICT;

-- `GET /p/{slug}` sources.
CREATE TABLE pixels (
    id           TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    slug         TEXT NOT NULL UNIQUE,
    lost_hits    INTEGER NOT NULL DEFAULT 0,
    created_at   INTEGER NOT NULL,
    deleted_at   INTEGER
) STRICT;
