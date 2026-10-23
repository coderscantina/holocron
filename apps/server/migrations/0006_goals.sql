-- Saved goals (milestone 07, decision 0003 Goals). SQLite, owned by the explorations module.
-- A goal's definition is versioned: every edit adds a row to `goal_versions` and moves
-- `goals.version`, earlier rows are never changed, so a report that copied a version keeps
-- its meaning. Definitions are the JSON text of the protocol `GoalDefinition` without `ref`.
-- New tables only: pending inboxes and existing backups are unaffected.

-- `name_key` is the case-folded name, unique per site among goals that are not archived.
CREATE TABLE goals (
    id          TEXT PRIMARY KEY,
    site_id     TEXT NOT NULL REFERENCES sites (id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    name_key    TEXT NOT NULL,
    version     INTEGER NOT NULL CHECK (version >= 1),
    archived_at INTEGER,
    created_by  TEXT NOT NULL,
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL
) STRICT;

CREATE UNIQUE INDEX goals_active_name ON goals (site_id, name_key) WHERE archived_at IS NULL;

CREATE TABLE goal_versions (
    goal_id    TEXT NOT NULL REFERENCES goals (id) ON DELETE CASCADE,
    version    INTEGER NOT NULL CHECK (version >= 1),
    definition TEXT NOT NULL,
    created_by TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (goal_id, version)
) STRICT;
