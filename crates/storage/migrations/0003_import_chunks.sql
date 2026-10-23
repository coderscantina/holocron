-- Import chunk ledger (milestone 11). An importer admits each chunk of historical records under
-- a deterministic key in the same transaction as the records, so a chunk retried after a crash
-- is recognized and never admitted twice. Rows stay after the records retire. New table only:
-- pending inboxes and existing backups are unaffected.
CREATE TABLE import_chunks (
    key TEXT PRIMARY KEY,
    first_seq INTEGER,
    last_seq INTEGER,
    records INTEGER NOT NULL,
    admitted_at INTEGER NOT NULL
) WITHOUT ROWID;
