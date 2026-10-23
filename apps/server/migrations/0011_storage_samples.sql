-- Storage samples for the operator status and its storage forecast (dashboard v2 v07). Control
-- database (SQLite) only, owned by maintenance. One new table: pending inboxes, analytical
-- storage and existing backup manifests are unaffected, and a backup restored into this release
-- starts without samples. It raises the control schema to 11, so an existing data directory
-- runs `holocron upgrade` first, as for every control migration (0014). The server samples at
-- start and then daily; retention keeps 400 days of rows.
--
-- Sizes are bytes, each file with its write-ahead log. `retained_records` and `ingest_per_day`
-- come from DuckDB `day_aggregates` (eligible events): the retained total and the median per
-- day over the last 28 days. `oldest_day` is the oldest retained local date in days since
-- 1970-01-01, NULL without records. No site, event or identity data is stored.
CREATE TABLE storage_samples (
    taken_at         INTEGER PRIMARY KEY,
    control_bytes    INTEGER NOT NULL,
    inbox_bytes      INTEGER NOT NULL,
    recordings_bytes INTEGER NOT NULL,
    analytics_bytes  INTEGER NOT NULL,
    spill_bytes      INTEGER NOT NULL,
    free_bytes       INTEGER NOT NULL,
    retained_records INTEGER NOT NULL,
    ingest_per_day   INTEGER NOT NULL,
    oldest_day       INTEGER
) STRICT;
