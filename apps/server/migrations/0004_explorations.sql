-- Exploration definitions (milestone 06): saved explorations, segments, annotations and boards.
-- States and filters are stored as the JSON text of the protocol types (decision 0004), so a
-- reopened definition is byte-for-byte what was saved. Analytics stores nothing here.

-- Personal saved explorations: visible to their owner only, on one site.
CREATE TABLE saved_explorations (
    id         TEXT PRIMARY KEY,
    site_id    TEXT NOT NULL REFERENCES sites (id) ON DELETE CASCADE,
    owner_id   TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    name       TEXT NOT NULL,
    state      TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
) STRICT;

CREATE INDEX saved_explorations_owner ON saved_explorations (owner_id, site_id);

-- Named filter groups shared with everyone who can read the site. `name_key` is the case-folded
-- name, unique per site (0004 Segments).
CREATE TABLE segments (
    id         TEXT PRIMARY KEY,
    site_id    TEXT NOT NULL REFERENCES sites (id) ON DELETE CASCADE,
    name       TEXT NOT NULL,
    name_key   TEXT NOT NULL,
    filter     TEXT NOT NULL,
    created_by TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    UNIQUE (site_id, name_key)
) STRICT;

CREATE TABLE annotations (
    id         TEXT PRIMARY KEY,
    site_id    TEXT NOT NULL REFERENCES sites (id) ON DELETE CASCADE,
    at         INTEGER NOT NULL,
    text       TEXT NOT NULL,
    created_by TEXT NOT NULL,
    created_at INTEGER NOT NULL
) STRICT;

CREATE INDEX annotations_site_at ON annotations (site_id, at);

-- Workspace boards. Cards hold a complete state of a site in the board's workspace.
CREATE TABLE boards (
    id           TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    created_by   TEXT NOT NULL,
    created_at   INTEGER NOT NULL,
    updated_at   INTEGER NOT NULL
) STRICT;

CREATE INDEX boards_workspace ON boards (workspace_id);

CREATE TABLE board_cards (
    id             TEXT PRIMARY KEY,
    board_id       TEXT NOT NULL REFERENCES boards (id) ON DELETE CASCADE,
    position       INTEGER NOT NULL,
    title          TEXT NOT NULL,
    site_id        TEXT NOT NULL REFERENCES sites (id) ON DELETE CASCADE,
    state          TEXT NOT NULL,
    exploration_id TEXT
) STRICT;

CREATE INDEX board_cards_board ON board_cards (board_id, position);
