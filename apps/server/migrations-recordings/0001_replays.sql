-- Replay store, `recordings.sqlite` (milestone 09, decision 0011). Captured replays live apart
-- from the event inbox and analytics, so their volume and cap never touch ordinary collection.
-- IDs are 16-byte big-endian UUIDs, times Unix milliseconds.

-- One capture per site and visit of the recorder's session token. `visitor` is the daily
-- pseudonym the token named; analytics links it to derived sessions at read time, so session
-- corrections never move a replay. `started_at` and `ended_at` come from the browser's clock.
-- `excluded` marks a capture that reached an excluded page: its parts were deleted and later
-- ones are dropped.
CREATE TABLE captures (
    site             BLOB NOT NULL,
    capture          BLOB NOT NULL,
    visitor          BLOB,
    received_at      INTEGER NOT NULL,
    last_received_at INTEGER NOT NULL,
    started_at       INTEGER NOT NULL,
    ended_at         INTEGER NOT NULL,
    first_chunk      INTEGER NOT NULL,
    chunks           INTEGER NOT NULL,
    events           INTEGER NOT NULL,
    bytes            INTEGER NOT NULL,
    entry_path       TEXT,
    pages            INTEGER NOT NULL,
    browser          TEXT,
    os               TEXT,
    device           TEXT,
    country          TEXT,
    truncated        INTEGER NOT NULL DEFAULT 0,
    excluded         INTEGER NOT NULL DEFAULT 0,
    saved_at         INTEGER,
    PRIMARY KEY (site, capture)
) STRICT;

CREATE INDEX captures_listed ON captures (site, received_at);
CREATE INDEX captures_saved ON captures (saved_at) WHERE saved_at IS NOT NULL;

-- One row per recorder request. `chunk` is the recorder's strictly increasing part index (Unix
-- seconds), so playback order never depends on arrival order. Two tabs of one visit can send
-- the same index; their parts both stay, in arrival order. The same bytes at the same index are
-- a repeated request and stored once. `payload` is the zstd-compressed JSON array of events as
-- sent; `fingerprint` the first 16 bytes of its BLAKE3 hash.
CREATE TABLE chunks (
    id          INTEGER PRIMARY KEY,
    site        BLOB NOT NULL,
    capture     BLOB NOT NULL,
    chunk       INTEGER NOT NULL,
    received_at INTEGER NOT NULL,
    events      INTEGER NOT NULL,
    raw_bytes   INTEGER NOT NULL,
    fingerprint BLOB NOT NULL,
    payload     BLOB NOT NULL
) STRICT;

CREATE UNIQUE INDEX chunks_order ON chunks (site, capture, chunk, fingerprint);

-- Paths a capture visited, for filtering and report links.
CREATE TABLE capture_pages (
    site    BLOB NOT NULL,
    capture BLOB NOT NULL,
    path    TEXT NOT NULL,
    PRIMARY KEY (site, capture, path)
) STRICT, WITHOUT ROWID;

-- Compressed bytes of every stored part, kept in the same transactions as the parts so the cap
-- check at admission is exact.
CREATE TABLE usage (
    id    INTEGER PRIMARY KEY CHECK (id = 1),
    bytes INTEGER NOT NULL
) STRICT;

INSERT INTO usage (id, bytes) VALUES (1, 0);
