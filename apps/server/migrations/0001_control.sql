-- Control database: accounts, sessions, workspaces, sites, topic events and idempotency records.
-- Timestamps are Unix milliseconds. IDs are UUID strings.

CREATE TABLE users (
    id            TEXT PRIMARY KEY,
    email         TEXT NOT NULL UNIQUE,
    name          TEXT NOT NULL,
    password_hash TEXT NOT NULL,
    install_role  TEXT NOT NULL CHECK (install_role IN ('user', 'install_admin')),
    created_at    INTEGER NOT NULL
) STRICT;

-- `id` is the SHA-256 of the cookie token, so a leaked database does not leak live sessions.
CREATE TABLE sessions (
    id         TEXT PRIMARY KEY,
    user_id    TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL
) STRICT;

CREATE INDEX sessions_user ON sessions (user_id);

CREATE TABLE workspaces (
    id         TEXT PRIMARY KEY,
    name       TEXT NOT NULL,
    kind       TEXT NOT NULL CHECK (kind IN ('personal', 'team')),
    created_at INTEGER NOT NULL
) STRICT;

CREATE TABLE memberships (
    workspace_id TEXT NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    user_id      TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    role         TEXT NOT NULL CHECK (role IN ('owner', 'admin', 'member', 'viewer')),
    PRIMARY KEY (workspace_id, user_id)
) STRICT;

CREATE INDEX memberships_user ON memberships (user_id);

CREATE TABLE sites (
    id           TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    domain       TEXT NOT NULL,
    timezone     TEXT NOT NULL,
    created_at   INTEGER NOT NULL
) STRICT;

CREATE INDEX sites_workspace ON sites (workspace_id);

-- One ordered log per topic (`installation`, `site:<id>`). A command's revision is the
-- sequence of the last event it recorded.
CREATE TABLE topic_events (
    topic      TEXT NOT NULL,
    seq        INTEGER NOT NULL,
    kind       TEXT NOT NULL,
    payload    TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (topic, seq)
) STRICT;

-- Committed command answers, replayable for 24 hours. `authorized` lists the site scopes the
-- command resolved, so a replay is authorized again before it is returned.
CREATE TABLE idempotency_keys (
    owner       TEXT NOT NULL,
    key         TEXT NOT NULL,
    operation   TEXT NOT NULL,
    fingerprint TEXT NOT NULL,
    response    TEXT NOT NULL,
    authorized  TEXT NOT NULL,
    created_at  INTEGER NOT NULL,
    PRIMARY KEY (owner, key)
) STRICT;

CREATE INDEX idempotency_keys_created ON idempotency_keys (created_at);
