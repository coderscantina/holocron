-- Operations (milestone 12, decision 0014): maintenance jobs.
--
-- Site deletion and reset commit their job here with the control change; operator commands
-- (`holocron backup`, `retention`, `erase-profile`, `diagnostics`) queue one while a server
-- runs and wait for its row. The server runs jobs one at a time, and an interrupted erasure or
-- retention job runs again at the next start; every step of those checks what is already done.
-- `args` and `result` are JSON. They never hold raw identity inputs: a profile is named by
-- its keyed ID.
CREATE TABLE maintenance_jobs (
    id          TEXT PRIMARY KEY,
    kind        TEXT NOT NULL CHECK (kind IN (
                    'erase_site', 'reset_site', 'erase_profile', 'retention', 'backup', 'diagnostics')),
    args        TEXT NOT NULL,
    state       TEXT NOT NULL CHECK (state IN ('queued', 'running', 'done', 'failed')),
    result      TEXT,
    created_at  INTEGER NOT NULL,
    started_at  INTEGER,
    finished_at INTEGER
) STRICT;

CREATE INDEX maintenance_jobs_open ON maintenance_jobs (created_at) WHERE state IN ('queued', 'running');
