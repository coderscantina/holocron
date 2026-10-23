-- Replay and heatmaps (milestone 09): the recording policy of a site. A site without a row
-- records nothing (CAP-RECORD-01). Masking is always strict and is not stored. Captured replays
-- live in their own database file, `recordings.sqlite`, not here (decision 0011).

-- `excluded_elements` and `excluded_paths` are JSON arrays of strings. `version` counts changes.
CREATE TABLE recording_policies (
    site_id                TEXT PRIMARY KEY REFERENCES sites (id) ON DELETE CASCADE,
    replay_enabled         INTEGER NOT NULL CHECK (replay_enabled IN (0, 1)),
    heatmap_enabled        INTEGER NOT NULL CHECK (heatmap_enabled IN (0, 1)),
    replay_sample_percent  INTEGER NOT NULL CHECK (replay_sample_percent BETWEEN 1 AND 100),
    heatmap_sample_percent INTEGER NOT NULL CHECK (heatmap_sample_percent BETWEEN 1 AND 100),
    max_duration_minutes   INTEGER NOT NULL CHECK (max_duration_minutes BETWEEN 1 AND 60),
    excluded_elements      TEXT NOT NULL,
    excluded_paths         TEXT NOT NULL,
    version                INTEGER NOT NULL CHECK (version >= 1),
    updated_at             INTEGER NOT NULL
) STRICT;
