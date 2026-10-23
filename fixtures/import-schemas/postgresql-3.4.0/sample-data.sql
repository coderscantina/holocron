-- Synthetic sample rows for the PostgreSQL 3.4.0 schema snapshot. Not from any real installation.
-- Loads into a database created from schema.sql or by migrating with the upstream 26 migrations.
-- UUID scheme: 00000000-0000-4000-8000-<kind><nn>, kind 1 users/teams, 2 websites, 3 sessions,
-- 4 events, 5 properties, 6 saved objects, 7 links/pixels/shares, 8 replay/heatmap, a visits.
begin;

insert into "user" (user_id, username, password, role, display_name, two_factor_required, created_at, updated_at) values
 ('00000000-0000-4000-8000-000000000101', 'admin',  '$2b$10$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA', 'admin', 'Sample Admin', false, '2026-01-01 08:00:00+00', '2026-01-01 08:00:00+00'),
 ('00000000-0000-4000-8000-000000000102', 'member', '$2b$10$BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB', 'user',  null,           true,  '2026-01-02 08:00:00+00', '2026-01-02 08:00:00+00');

insert into two_factor_auth (id, user_id, secret, is_enabled, created_at, updated_at) values
 ('tfa-sample-1', '00000000-0000-4000-8000-000000000102', 'synthetic-encrypted-secret', true, '2026-01-03 08:00:00+00', '2026-01-03 08:00:00+00');
insert into two_factor_backup_code (id, user_id, code_hash, used, created_at) values
 ('tfb-sample-1', '00000000-0000-4000-8000-000000000102', 'synthetic-code-hash', false, '2026-01-03 08:00:00+00');

insert into api_key (api_key_id, user_id, name, key_hash, key_prefix, created_at) values
 ('00000000-0000-4000-8000-000000000141', '00000000-0000-4000-8000-000000000101', 'sample key', 'synthetic-key-hash-0001', 'um_sample', '2026-01-04 08:00:00+00');

insert into team (team_id, name, access_code, two_factor_required, created_at, updated_at) values
 ('00000000-0000-4000-8000-000000000110', 'Sample Team', 'team-sample-code', false, '2026-01-05 08:00:00+00', '2026-01-05 08:00:00+00');
insert into team_user (team_user_id, team_id, user_id, role, created_at, updated_at) values
 ('00000000-0000-4000-8000-000000000111', '00000000-0000-4000-8000-000000000110', '00000000-0000-4000-8000-000000000101', 'team-owner',  '2026-01-05 08:00:00+00', '2026-01-05 08:00:00+00'),
 ('00000000-0000-4000-8000-000000000112', '00000000-0000-4000-8000-000000000110', '00000000-0000-4000-8000-000000000102', 'team-member', '2026-01-05 09:00:00+00', '2026-01-05 09:00:00+00');

insert into website (website_id, name, domain, user_id, created_by, team_id, recorder_enabled, replay_config, created_at, updated_at) values
 ('00000000-0000-4000-8000-000000000201', 'Sample Site', 'example.test', '00000000-0000-4000-8000-000000000101', '00000000-0000-4000-8000-000000000101', null, false, null, '2026-01-06 08:00:00+00', '2026-01-06 08:00:00+00'),
 ('00000000-0000-4000-8000-000000000202', 'Team Site',   'team.example.test', null, '00000000-0000-4000-8000-000000000102', '00000000-0000-4000-8000-000000000110', true, '{"sampleRate": 1}', '2026-01-06 09:00:00+00', '2026-01-06 09:00:00+00');

insert into session (session_id, website_id, browser, os, device, screen, language, country, region, city, distinct_id, created_at) values
 ('00000000-0000-4000-8000-000000000301', '00000000-0000-4000-8000-000000000201', 'chrome',  'Windows 10', 'desktop', '1920x1080', 'en-US', 'US', 'US-CA', 'Sampleville', 'sample-user-1', '2026-02-01 10:00:00+00'),
 ('00000000-0000-4000-8000-000000000302', '00000000-0000-4000-8000-000000000202', 'firefox', 'Linux',      'laptop',  '1366x768',  'de-AT', 'AT', 'AT-9',  'Wien',        null,            '2026-02-01 11:00:00+00');

insert into website_event (event_id, website_id, session_id, visit_id, created_at, url_path, url_query, referrer_path, referrer_domain, page_title, event_type, event_name, hostname, utm_source, utm_medium, utm_campaign, lcp, cls, ttfb) values
 ('00000000-0000-4000-8000-000000000401', '00000000-0000-4000-8000-000000000201', '00000000-0000-4000-8000-000000000301', '00000000-0000-4000-8000-000000000a01', '2026-02-01 10:00:00+00', '/',        '',         '/search', 'search.test', 'Home',     1, null,       'example.test', 'newsletter', 'email', 'spring', null, null, null),
 ('00000000-0000-4000-8000-000000000402', '00000000-0000-4000-8000-000000000201', '00000000-0000-4000-8000-000000000301', '00000000-0000-4000-8000-000000000a01', '2026-02-01 10:01:00+00', '/pricing', null,       null,      null,          'Pricing',  2, 'purchase', 'example.test', null, null, null, null, null, null),
 ('00000000-0000-4000-8000-000000000403', '00000000-0000-4000-8000-000000000201', '00000000-0000-4000-8000-000000000301', '00000000-0000-4000-8000-000000000a01', '2026-02-01 10:01:30+00', '/pricing', null,       null,      null,          null,       5, null,       'example.test', null, null, null, 1800.0, 0.0500, 120.0),
 ('00000000-0000-4000-8000-000000000404', '00000000-0000-4000-8000-000000000202', '00000000-0000-4000-8000-000000000302', '00000000-0000-4000-8000-000000000a02', '2026-02-01 11:00:00+00', '/docs',    'q=sample', null,      null,          'Docs',     1, null,       'team.example.test', null, null, null, null, null, null);

insert into event_data (event_data_id, website_id, website_event_id, data_key, string_value, number_value, date_value, data_type, created_at) values
 ('00000000-0000-4000-8000-000000000501', '00000000-0000-4000-8000-000000000201', '00000000-0000-4000-8000-000000000402', 'plan',     'pro',   null,    null, 1, '2026-02-01 10:01:00+00'),
 ('00000000-0000-4000-8000-000000000502', '00000000-0000-4000-8000-000000000201', '00000000-0000-4000-8000-000000000402', 'revenue',  null,    49.9000, null, 2, '2026-02-01 10:01:00+00'),
 ('00000000-0000-4000-8000-000000000503', '00000000-0000-4000-8000-000000000201', '00000000-0000-4000-8000-000000000402', 'currency', 'EUR',   null,    null, 1, '2026-02-01 10:01:00+00');
insert into session_data (session_data_id, website_id, session_id, data_key, string_value, number_value, date_value, data_type, distinct_id, created_at) values
 ('00000000-0000-4000-8000-000000000511', '00000000-0000-4000-8000-000000000201', '00000000-0000-4000-8000-000000000301', 'plan', 'pro', null, null, 1, 'sample-user-1', '2026-02-01 10:00:00+00');
insert into session_link (website_id, session_id, distinct_id, created_at) values
 ('00000000-0000-4000-8000-000000000201', '00000000-0000-4000-8000-000000000301', 'sample-user-1', '2026-02-01 10:00:00+00');

insert into revenue (revenue_id, website_id, session_id, event_id, event_name, currency, revenue, created_at) values
 ('00000000-0000-4000-8000-000000000521', '00000000-0000-4000-8000-000000000201', '00000000-0000-4000-8000-000000000301', '00000000-0000-4000-8000-000000000402', 'purchase', 'EUR', 49.9000, '2026-02-01 10:01:00+00');

insert into report (report_id, user_id, website_id, type, name, description, parameters, created_at, updated_at) values
 ('00000000-0000-4000-8000-000000000601', '00000000-0000-4000-8000-000000000101', '00000000-0000-4000-8000-000000000201', 'funnel', 'Sample funnel', 'Home to pricing', '{"window": 60, "steps": [{"type": "path", "value": "/"}, {"type": "path", "value": "/pricing"}]}', '2026-02-02 08:00:00+00', '2026-02-02 08:00:00+00');
insert into segment (segment_id, website_id, type, name, parameters, created_at, updated_at) values
 ('00000000-0000-4000-8000-000000000611', '00000000-0000-4000-8000-000000000201', 'segment', 'Desktop', '{"filters": [{"name": "device", "operator": "eq", "value": "desktop"}]}', '2026-02-02 08:00:00+00', '2026-02-02 08:00:00+00');
insert into board (board_id, type, name, description, parameters, user_id, team_id, created_at, updated_at) values
 ('00000000-0000-4000-8000-000000000621', 'website', 'Sample board', 'One panel', '{"rows": []}', '00000000-0000-4000-8000-000000000101', null, '2026-02-02 08:00:00+00', '2026-02-02 08:00:00+00');
insert into annotation (annotation_id, website_id, user_id, date, all_day, note, created_at, updated_at) values
 ('00000000-0000-4000-8000-000000000631', '00000000-0000-4000-8000-000000000201', '00000000-0000-4000-8000-000000000101', '2026-02-01 00:00:00+00', true, 'Sample launch', '2026-02-02 08:00:00+00', '2026-02-02 08:00:00+00');

insert into link (link_id, name, url, slug, user_id, created_at, updated_at) values
 ('00000000-0000-4000-8000-000000000701', 'Sample link', 'https://example.test/target', 'sample-link', '00000000-0000-4000-8000-000000000101', '2026-02-03 08:00:00+00', '2026-02-03 08:00:00+00');
insert into pixel (pixel_id, name, slug, user_id, created_at, updated_at) values
 ('00000000-0000-4000-8000-000000000711', 'Sample pixel', 'sample-pixel', '00000000-0000-4000-8000-000000000101', '2026-02-03 08:00:00+00', '2026-02-03 08:00:00+00');
insert into share (share_id, entity_id, name, share_type, slug, parameters, created_at, updated_at) values
 ('00000000-0000-4000-8000-000000000721', '00000000-0000-4000-8000-000000000201', 'Public view', 1, 'sample-share', '{"overview": true}', '2026-02-03 08:00:00+00', '2026-02-03 08:00:00+00');

insert into session_replay (replay_id, website_id, session_id, visit_id, chunk_index, events, event_count, started_at, ended_at, created_at) values
 ('00000000-0000-4000-8000-000000000801', '00000000-0000-4000-8000-000000000202', '00000000-0000-4000-8000-000000000302', '00000000-0000-4000-8000-000000000a02', 0, '\x7b7d', 1, '2026-02-01 11:00:00+00', '2026-02-01 11:00:30+00', '2026-02-01 11:00:30+00');
insert into session_replay_saved (saved_replay_id, name, website_id, visit_id, created_at, updated_at) values
 ('00000000-0000-4000-8000-000000000811', 'Sample replay', '00000000-0000-4000-8000-000000000202', '00000000-0000-4000-8000-000000000a02', '2026-02-04 08:00:00+00', '2026-02-04 08:00:00+00');
insert into heatmap_event (heatmap_event_id, website_id, session_id, visit_id, url_path, event_type, x, y, page_x, page_y, page_w, viewport_w, viewport_h, page_h, scroll_pct, created_at) values
 ('00000000-0000-4000-8000-000000000821', '00000000-0000-4000-8000-000000000202', '00000000-0000-4000-8000-000000000302', '00000000-0000-4000-8000-000000000a02', '/docs', 1, 100, 200, 100, 600, 1280, 1280, 720, 3000, null, '2026-02-01 11:00:10+00');

insert into app_setting (key, value) values ('sample', 'true');
commit;
