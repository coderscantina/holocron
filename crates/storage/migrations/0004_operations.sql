-- Operations (milestone 12, decision 0014): the pairing ID shared with analytics.duckdb, and
-- erasure fences.

-- One random ID, written into this file and into analytics.duckdb when the pair is created.
-- Opening an inbox with an analytics file of another pair is refused.
CREATE TABLE pairing (
    id   INTEGER PRIMARY KEY CHECK (id = 1),
    pair TEXT NOT NULL
);

-- Erasure fences. Transfer never applies a record a fence covers, and every other writer into
-- analytics (imports) checks the same fences. `profile` NULL covers the whole site.
-- `forever` covers records of the site admitted at any time (site deletion); otherwise the
-- fence covers records admitted up to `through_seq`, and imports started before `created_at`.
-- `key` names the job that asked for the fence, so asking again returns the same fence.
CREATE TABLE fences (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    key         TEXT NOT NULL UNIQUE,
    site        BLOB NOT NULL,
    profile     BLOB,
    forever     INTEGER NOT NULL CHECK (forever IN (0, 1)),
    through_seq INTEGER NOT NULL,
    created_at  INTEGER NOT NULL
);
