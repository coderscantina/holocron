-- Collection keys and persistent collection counters. Control database only, owned by the
-- collection module. New tables only: pending inboxes, analytical storage and existing backups
-- are unaffected.

-- Site collection keys of trusted server integrations (decision 0001). A request carrying one
-- may set `payload.ip` and `payload.userAgent` for its site. The secret is stored as a SHA-256
-- hex digest like API keys (0012), so a lookup is one index probe. Revoked keys stay listed;
-- deleting the site removes its keys.
CREATE TABLE collection_keys (
    id          TEXT PRIMARY KEY,
    site_id     TEXT NOT NULL REFERENCES sites (id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    prefix      TEXT NOT NULL,
    secret_hash TEXT NOT NULL UNIQUE,
    created_at  INTEGER NOT NULL,
    revoked_at  INTEGER
) STRICT;

CREATE INDEX collection_keys_site ON collection_keys (site_id);

-- `collection.status` counts, written from memory every 10 seconds and at a clean stop, so a
-- restart keeps them. Each row holds the running total; `reason` is '' when there is none.
-- Values are the snake_case wire names of the protocol enums.
CREATE TABLE collection_outcome_counts (
    route   TEXT NOT NULL,
    outcome TEXT NOT NULL,
    reason  TEXT NOT NULL,
    count   INTEGER NOT NULL,
    PRIMARY KEY (route, outcome, reason)
) STRICT, WITHOUT ROWID;

CREATE TABLE collection_counter_counts (
    counter TEXT PRIMARY KEY,
    count   INTEGER NOT NULL
) STRICT, WITHOUT ROWID;

-- When counting started: the first start with this schema.
CREATE TABLE collection_counts_since (
    id    INTEGER PRIMARY KEY CHECK (id = 1),
    since INTEGER NOT NULL
) STRICT;
