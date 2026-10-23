-- Site summaries and noticed changes (dashboard v2 v02, decision 0015 Where it runs). Control
-- database (SQLite) only, owned by the summaries step in the server. Two new tables: pending
-- inboxes, analytical storage and existing backup manifests are unaffected. It raises the
-- control schema to 12, so an existing data directory runs `holocron upgrade` first, as for
-- every control migration (0014). A backup taken before it restores without summaries; any
-- restored row is recomputed in the first round after start, since the step keeps its schedule
-- in memory.
--
-- Both tables are derived records with their own watermark, not report caches: the step
-- rewrites them from `analytics.explore` results, erasure and reset of a site delete them inside
-- the erasure job, and `summaries.forSites` reads them with one query. No foreign key to
-- `sites`: the erasure job removes them after the analytical erasure, and the step writes only
-- while the site exists and no erasure of it is pending.

-- One row per site: today so far against a typical weekday (0015, today with hour
-- granularity). `*_today` are exact counts up to `computed_at`; `*_typical` the typical weekday
-- up to the same local time, NULL with fewer than 4 baseline days (`insufficient_history`);
-- `*_pace` the relative change between them, NULL when either side is missing or the typical
-- value is 0. `*_projection*` is the median projection of the day with its 25th to 75th
-- percentile band and the basis days it came from, NULL without enough history.
-- `hourly_today` and `hourly_typical` are JSON arrays of pageviews per local hour of today
-- (23 to 25 entries on a daylight saving day); typical entries are `null` without history.
-- `watermark_*` is the analytical position the row was computed at, `metric_version` the
-- formula version (0003). `finding_count` is the number of `site_findings` rows of the site.
CREATE TABLE site_summaries (
    site_id                  TEXT PRIMARY KEY,
    computed_at              INTEGER NOT NULL,
    watermark_batch          INTEGER NOT NULL,
    watermark_sequence       INTEGER NOT NULL,
    metric_version           INTEGER NOT NULL,
    timezone                 TEXT NOT NULL,
    pageviews_today          INTEGER NOT NULL,
    pageviews_typical        INTEGER,
    pageviews_pace           REAL,
    pageviews_projection     REAL,
    pageviews_projection_low REAL,
    pageviews_projection_high REAL,
    visitor_days_today       INTEGER NOT NULL,
    visitor_days_typical     INTEGER,
    visitor_days_pace        REAL,
    visitor_days_projection  REAL,
    visitor_days_projection_low REAL,
    visitor_days_projection_high REAL,
    projection_basis_days    INTEGER,
    hourly_today             TEXT NOT NULL,
    hourly_typical           TEXT NOT NULL,
    insufficient_history     INTEGER NOT NULL,
    finding_count            INTEGER NOT NULL
) STRICT;

-- A site's current findings, replaced as a set with its summary row in one transaction.
-- `rule_id` and `rule_version` name the rule that fired, `kind` the template the UI renders
-- (no text is stored), `measured` the values the template needs as JSON, `state` the
-- exploration state that shows the finding (0004 v1, validated like any saved state).
-- `window_start` is the start of the compared window, Unix milliseconds; `share` the finding's
-- share of the site's total change, 0 to 1. `rank` orders a site's findings from 1.
CREATE TABLE site_findings (
    site_id      TEXT NOT NULL,
    rank         INTEGER NOT NULL,
    rule_id      TEXT NOT NULL,
    rule_version INTEGER NOT NULL,
    kind         TEXT NOT NULL,
    measured     TEXT NOT NULL,
    state        TEXT NOT NULL,
    window_start INTEGER NOT NULL,
    share        REAL NOT NULL,
    computed_at  INTEGER NOT NULL,
    PRIMARY KEY (site_id, rank)
) STRICT, WITHOUT ROWID;
