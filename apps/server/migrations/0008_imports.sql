-- Historical import (milestone 11, decision 0013). Control database only, owned by the imports
-- module. New tables only: pending inboxes, analytical storage and existing backups are
-- unaffected.

-- The one import of this data directory. A row with `state = 'running'` means an import is in
-- progress or was interrupted and resumes with the same source and options; `serve` refuses to
-- start until it is complete. `clock_ms` is pinned at the first start, so every retention
-- cutoff, share expiry and resume uses the same instant. `fingerprint` is the JSON of the source
-- row counts and newest timestamps: a resume against a changed source is refused.
CREATE TABLE import_runs (
    id          INTEGER PRIMARY KEY CHECK (id = 1),
    source      TEXT NOT NULL,
    fingerprint TEXT NOT NULL,
    options     TEXT NOT NULL,
    clock_ms    INTEGER NOT NULL,
    state       TEXT NOT NULL CHECK (state IN ('running', 'complete')),
    started_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL,
    finished_at INTEGER,
    manifest    TEXT
) STRICT;

-- Checkpoint of each step. `cursor` is the last source key a step committed and `chunks` the
-- number of chunks it committed; chunk `n` of step `s` is admitted to the inbox under the key
-- `s:n`, so a chunk retried after a crash is recognized there. `done` marks a finished step.
CREATE TABLE import_steps (
    step       TEXT PRIMARY KEY,
    cursor     TEXT,
    chunks     INTEGER NOT NULL DEFAULT 0,
    rows       INTEGER NOT NULL DEFAULT 0,
    imported   INTEGER NOT NULL DEFAULT 0,
    skipped    INTEGER NOT NULL DEFAULT 0,
    done       INTEGER NOT NULL DEFAULT 0 CHECK (done IN (0, 1)),
    updated_at INTEGER NOT NULL
) STRICT;

-- Source objects not imported, or imported with a loss, kept with their source representation
-- (secrets redacted) and the reason. Written in the same transaction as the step that read them.
CREATE TABLE import_inventory (
    source_table   TEXT NOT NULL,
    source_key     TEXT NOT NULL,
    reason         TEXT NOT NULL,
    representation TEXT NOT NULL,
    PRIMARY KEY (source_table, source_key, reason)
) STRICT;
