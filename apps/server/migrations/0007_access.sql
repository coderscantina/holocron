-- Access and restricted sharing (milestone 10, decisions 0007 and 0012). Control database only,
-- owned by the identity module (sessions, 2FA, keys, users, policy, preferences) and the access
-- module (teams, invitations, shares). New tables and nullable or defaulted columns only:
-- pending inboxes, analytical storage and existing backups are unaffected.
--
-- Every token, key secret, invitation code and recovery code is stored as a SHA-256 hex digest,
-- as session IDs already are: they carry at least 80 random bits, so a leaked database does not
-- leak a usable credential and a lookup is one index probe. Whether a session is enrollment-only
-- is not stored: it is derived on every request from the policies below and the 2FA row, so a
-- requirement switched on applies at the next request and confirming enrollment upgrades the
-- same session (decision 0007).

-- `reauthenticated_at` is the last passed password check of this login session; sensitive
-- commands need it within 10 minutes. `last_seen_at` drives the idle timeout and is written at
-- most once a minute, so reads do not queue on the single writer.
ALTER TABLE sessions ADD COLUMN reauthenticated_at INTEGER;
ALTER TABLE sessions ADD COLUMN last_seen_at INTEGER NOT NULL DEFAULT 0;
UPDATE sessions SET last_seen_at = created_at;

-- The step between a correct password and the second factor. `id` is the digest of the
-- challenge cookie. Never a session: it reaches only `auth.verifySecondFactor`.
CREATE TABLE login_challenges (
    id         TEXT PRIMARY KEY,
    user_id    TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    attempts   INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL
) STRICT;

CREATE INDEX login_challenges_user ON login_challenges (user_id);

-- One second factor per person. Secrets are AEAD ciphertexts (nonce, ciphertext and tag) under
-- the key in the data directory's `secrets.key`, never under a key stored in this database
-- (0012). `secret` is the confirmed factor; `pending_secret` waits for `twoFactor.confirm` and
-- leaves an active factor working meanwhile. `last_step` is the newest accepted time step, so
-- a code is accepted once.
CREATE TABLE two_factor (
    user_id            TEXT PRIMARY KEY REFERENCES users (id) ON DELETE CASCADE,
    secret             BLOB,
    confirmed_at       INTEGER,
    last_step          INTEGER,
    pending_secret     BLOB,
    pending_created_at INTEGER,
    CHECK ((secret IS NULL) = (confirmed_at IS NULL)),
    CHECK ((pending_secret IS NULL) = (pending_created_at IS NULL))
) STRICT;

-- Single-use recovery codes. `used_at` marks a spent code; regeneration replaces the set.
CREATE TABLE recovery_codes (
    user_id   TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    code_hash TEXT NOT NULL,
    used_at   INTEGER,
    PRIMARY KEY (user_id, code_hash)
) STRICT, WITHOUT ROWID;

-- Scoped API keys. A key acts as its owner, limited to the operations and sites below and to
-- the owner's current capabilities. Revoked keys stay listed until the owner is erased.
CREATE TABLE api_keys (
    id           TEXT PRIMARY KEY,
    owner_id     TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    prefix       TEXT NOT NULL,
    secret_hash  TEXT NOT NULL UNIQUE,
    all_sites    INTEGER NOT NULL CHECK (all_sites IN (0, 1)),
    created_at   INTEGER NOT NULL,
    last_used_at INTEGER,
    revoked_at   INTEGER
) STRICT;

CREATE INDEX api_keys_owner ON api_keys (owner_id);

CREATE TABLE api_key_operations (
    key_id    TEXT NOT NULL REFERENCES api_keys (id) ON DELETE CASCADE,
    operation TEXT NOT NULL,
    PRIMARY KEY (key_id, operation)
) STRICT, WITHOUT ROWID;

-- Listed sites of keys with `all_sites = 0`. Deleting a site removes it from every key.
CREATE TABLE api_key_sites (
    key_id  TEXT NOT NULL REFERENCES api_keys (id) ON DELETE CASCADE,
    site_id TEXT NOT NULL REFERENCES sites (id) ON DELETE CASCADE,
    PRIMARY KEY (key_id, site_id)
) STRICT, WITHOUT ROWID;

CREATE INDEX api_key_sites_site ON api_key_sites (site_id);

-- Team invitations without an email service: the admin passes the code on, a signed-in person
-- redeems it once. Invitations of an erased creator go with them (0007 User erasure scope).
CREATE TABLE invitations (
    id           TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    role         TEXT NOT NULL CHECK (role IN ('owner', 'admin', 'member', 'viewer')),
    label        TEXT NOT NULL,
    prefix       TEXT NOT NULL,
    code_hash    TEXT NOT NULL UNIQUE,
    created_by   TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    created_at   INTEGER NOT NULL,
    expires_at   INTEGER NOT NULL,
    used_by      TEXT REFERENCES users (id) ON DELETE SET NULL,
    used_at      INTEGER,
    revoked_at   INTEGER
) STRICT;

CREATE INDEX invitations_workspace ON invitations (workspace_id, created_at);

-- 2FA requirements (CAP-ACCESS-04). A person must use 2FA when the installation, any team they
-- belong to, or their own account requires it.
CREATE TABLE installation_settings (
    id                  INTEGER PRIMARY KEY CHECK (id = 1),
    two_factor_required INTEGER NOT NULL CHECK (two_factor_required IN (0, 1)),
    updated_at          INTEGER NOT NULL
) STRICT;

INSERT INTO installation_settings (id, two_factor_required, updated_at) VALUES (1, 0, 0);

ALTER TABLE workspaces ADD COLUMN two_factor_required INTEGER NOT NULL DEFAULT 0 CHECK (two_factor_required IN (0, 1));
ALTER TABLE users ADD COLUMN two_factor_required INTEGER NOT NULL DEFAULT 0 CHECK (two_factor_required IN (0, 1));

-- Restricted shares. `target` is the JSON text of the protocol `ShareTarget`, holding a copy of
-- the shared state and its limits (decision 0004 stores states as JSON text), so a share does
-- not depend on the saved exploration or its author. Exploration shares name their site, board
-- shares their board; both name the workspace, and a site that leaves it is outside the share.
-- `created_by` has no foreign key: shares outlive their author as "Deleted user" (0007).
CREATE TABLE shares (
    id           TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    site_id      TEXT REFERENCES sites (id) ON DELETE CASCADE,
    board_id     TEXT REFERENCES boards (id) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    target       TEXT NOT NULL,
    export       INTEGER NOT NULL CHECK (export IN (0, 1)),
    recordings   INTEGER NOT NULL CHECK (recordings IN (0, 1)),
    prefix       TEXT NOT NULL,
    token_hash   TEXT NOT NULL UNIQUE,
    created_by   TEXT NOT NULL,
    created_at   INTEGER NOT NULL,
    updated_at   INTEGER NOT NULL,
    expires_at   INTEGER NOT NULL,
    revoked_at   INTEGER,
    CHECK ((site_id IS NULL) <> (board_id IS NULL))
) STRICT;

CREATE INDEX shares_site ON shares (site_id) WHERE site_id IS NOT NULL;
CREATE INDEX shares_board ON shares (board_id) WHERE board_id IS NOT NULL;
CREATE INDEX shares_workspace ON shares (workspace_id);

-- What remains of an erased account: its ID and deletion time, so audit references and
-- "Deleted user" authors stay resolvable (0007 User erasure scope).
CREATE TABLE user_tombstones (
    user_id    TEXT PRIMARY KEY,
    deleted_at INTEGER NOT NULL
) STRICT;

-- Display preferences. `default_range` is the JSON text of an exploration `Range`.
CREATE TABLE user_preferences (
    user_id       TEXT PRIMARY KEY REFERENCES users (id) ON DELETE CASCADE,
    timezone      TEXT,
    default_range TEXT,
    locale        TEXT CHECK (locale IN ('en', 'de')),
    theme         TEXT CHECK (theme IN ('system', 'light', 'dark')),
    updated_at    INTEGER NOT NULL
) STRICT;

-- A stored answer of a command that passed a reauthentication check is returned only while
-- the session's check is still fresh (AX-S025).
ALTER TABLE idempotency_keys ADD COLUMN fresh_login INTEGER NOT NULL DEFAULT 0 CHECK (fresh_login IN (0, 1));
