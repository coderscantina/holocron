-- The fixed collection sources of seed.mjs in an initialized Holocron data directory, for runs
-- with UMAMI_URL pointing at Holocron. Apply to control.sqlite before `holocron serve`:
--   sqlite3 <data>/control.sqlite < tools/collection-harness/seed-holocron.sql
-- The first workspace owns the sources. Recording policies match seed.mjs; Holocron always
-- masks strictly.

INSERT OR IGNORE INTO sites (id, workspace_id, name, domain, created_at)
SELECT site.id, (SELECT id FROM workspaces ORDER BY created_at LIMIT 1), site.name, 'localhost', 0
FROM (SELECT '5e1a0000-0000-4000-8000-000000000001' AS id, 'holocron plain' AS name
      UNION ALL SELECT '5e1a0000-0000-4000-8000-000000000002', 'holocron recorder'
      UNION ALL SELECT '5e1a0000-0000-4000-8000-000000000003', 'holocron heatmap only'
      UNION ALL SELECT '5e1a0000-0000-4000-8000-000000000004', 'holocron replay only'
      UNION ALL SELECT '5e1a0000-0000-4000-8000-000000000005', 'holocron recorder private') AS site;

INSERT OR IGNORE INTO session_policies (site_id, version, epoch, effective_at, timezone, window_minutes, created_at)
SELECT id, 1, 1, -9223372036854775808, 'UTC', 30, 0 FROM sites WHERE id LIKE '5e1a0000-%';

INSERT OR IGNORE INTO recording_policies (site_id, replay_enabled, heatmap_enabled, replay_sample_percent,
    heatmap_sample_percent, max_duration_minutes, excluded_elements, excluded_paths, version, updated_at)
VALUES ('5e1a0000-0000-4000-8000-000000000002', 1, 1, 100, 100, 5, '[]', '[]', 1, 0),
       ('5e1a0000-0000-4000-8000-000000000003', 0, 1, 10, 100, 5, '[]', '[]', 1, 0),
       ('5e1a0000-0000-4000-8000-000000000004', 1, 0, 100, 10, 5, '[]', '[]', 1, 0),
       ('5e1a0000-0000-4000-8000-000000000005', 1, 0, 100, 10, 5, '[".private"]', '[]', 1, 0);

INSERT OR IGNORE INTO links (id, workspace_id, name, slug, url, created_at)
SELECT '5e1a0000-0000-4000-8000-0000000000a1', (SELECT id FROM workspaces ORDER BY created_at LIMIT 1),
       'holocron link', 'holocron-link', 'https://example.com/landing?from=holocron', 0;

INSERT OR IGNORE INTO pixels (id, workspace_id, name, slug, created_at)
SELECT '5e1a0000-0000-4000-8000-0000000000b1', (SELECT id FROM workspaces ORDER BY created_at LIMIT 1),
       'holocron pixel', 'holocron-pixel', 0;
