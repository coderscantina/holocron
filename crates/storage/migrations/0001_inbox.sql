-- Durable inbox: accepted records wait here until a DuckDB batch containing
-- them commits. AUTOINCREMENT keeps sequences and batch IDs from ever being
-- reused after retirement deletes the rows.

CREATE TABLE batches (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    first_seq INTEGER NOT NULL,
    last_seq INTEGER NOT NULL,
    records INTEGER NOT NULL,
    bytes INTEGER NOT NULL,
    assigned_at INTEGER NOT NULL
);

CREATE TABLE inbox (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    received_at INTEGER NOT NULL,
    format INTEGER NOT NULL,
    bytes INTEGER NOT NULL,
    payload BLOB NOT NULL,
    batch_id INTEGER REFERENCES batches (id)
);

CREATE INDEX inbox_pending ON inbox (seq) WHERE batch_id IS NULL;
CREATE INDEX inbox_batch ON inbox (batch_id) WHERE batch_id IS NOT NULL;

-- Client eventId deduplication, scope (site, eventId), 48 hours (0002).
CREATE TABLE client_event_ids (
    site BLOB NOT NULL,
    event_id TEXT NOT NULL,
    first_received_at INTEGER NOT NULL,
    PRIMARY KEY (site, event_id)
) WITHOUT ROWID;
