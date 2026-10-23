-- Identity, sessions and corrections (milestone 05). Control storage owns site settings
-- (decision 0008); analytics keeps a derived copy of the session policy versions, written
-- by one code path after each change and at startup (decision 0002, Policy ownership).

-- Every session policy version of a site (0002 Calculation and policy versions). `epoch`
-- counts timezone changes and scopes day keys (0001 Timezone change). Version 1 takes effect
-- at the smallest integer, so it applies to every earlier event time.
CREATE TABLE session_policies (
    site_id        TEXT NOT NULL REFERENCES sites (id) ON DELETE CASCADE,
    version        INTEGER NOT NULL CHECK (version >= 1),
    epoch          INTEGER NOT NULL CHECK (epoch >= 1),
    effective_at   INTEGER NOT NULL,
    timezone       TEXT NOT NULL,
    window_minutes INTEGER NOT NULL CHECK (window_minutes BETWEEN 1 AND 1440),
    created_at     INTEGER NOT NULL,
    PRIMARY KEY (site_id, version)
) STRICT;

INSERT INTO session_policies (site_id, version, epoch, effective_at, timezone, window_minutes, created_at)
SELECT id, 1, timezone_epoch, -9223372036854775808, timezone, 30, created_at FROM sites;

-- The policy versions above are now the only record of a site's timezone and epoch.
ALTER TABLE sites DROP COLUMN timezone_epoch;
ALTER TABLE sites DROP COLUMN timezone;

-- Persistent identified analytics, opt-in per site (0001). The version counts changes.
ALTER TABLE sites ADD COLUMN persistent_identity INTEGER NOT NULL DEFAULT 0 CHECK (persistent_identity IN (0, 1));
ALTER TABLE sites ADD COLUMN identity_policy_version INTEGER NOT NULL DEFAULT 1;

-- Consent intervals of persistent profiles (0001 Persistent consent transport). An event links
-- to its profile when `granted_at <= event_time < revoked_at`. At most one interval is open.
CREATE TABLE consents (
    site_id    TEXT NOT NULL REFERENCES sites (id) ON DELETE CASCADE,
    profile_id BLOB NOT NULL CHECK (length(profile_id) = 16),
    granted_at INTEGER NOT NULL,
    revoked_at INTEGER,
    PRIMARY KEY (site_id, profile_id, granted_at)
) STRICT, WITHOUT ROWID;

CREATE UNIQUE INDEX consents_open ON consents (site_id, profile_id) WHERE revoked_at IS NULL;

-- Requested recalculations. The days are local dates; `through_batch` is the applied batch
-- their dirty rows were marked at, so the job is done when none of those rows is left.
CREATE TABLE recalculations (
    id            TEXT PRIMARY KEY,
    site_id       TEXT NOT NULL REFERENCES sites (id) ON DELETE CASCADE,
    first_day     TEXT NOT NULL,
    last_day      TEXT NOT NULL,
    through_batch INTEGER NOT NULL,
    requested_by  TEXT NOT NULL,
    requested_at  INTEGER NOT NULL
) STRICT;

CREATE INDEX recalculations_site ON recalculations (site_id);
