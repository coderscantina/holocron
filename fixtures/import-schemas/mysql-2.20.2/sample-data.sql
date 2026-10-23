-- Synthetic sample rows for the MySQL 2.20.2 schema snapshot. Not from any real installation.
-- Ids are 36-character UUID strings. Timestamps are UTC with whole seconds (timestamp(0)).
set time_zone = '+00:00';
start transaction;

insert into `user` (user_id, username, password, role, display_name, created_at, updated_at) values
 ('00000000-0000-4000-8000-000000000101', 'admin',  '$2b$10$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA', 'admin', 'Sample Admin', '2024-01-01 08:00:00', '2024-01-01 08:00:00'),
 ('00000000-0000-4000-8000-000000000102', 'member', '$2b$10$BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB', 'user',  null,           '2024-01-02 08:00:00', '2024-01-02 08:00:00');

insert into team (team_id, name, access_code, created_at, updated_at) values
 ('00000000-0000-4000-8000-000000000110', 'Sample Team', 'team-sample-code', '2024-01-05 08:00:00', '2024-01-05 08:00:00');
insert into team_user (team_user_id, team_id, user_id, role, created_at, updated_at) values
 ('00000000-0000-4000-8000-000000000111', '00000000-0000-4000-8000-000000000110', '00000000-0000-4000-8000-000000000101', 'team-owner',  '2024-01-05 08:00:00', '2024-01-05 08:00:00'),
 ('00000000-0000-4000-8000-000000000112', '00000000-0000-4000-8000-000000000110', '00000000-0000-4000-8000-000000000102', 'team-member', '2024-01-05 09:00:00', '2024-01-05 09:00:00');

insert into website (website_id, name, domain, share_id, user_id, created_by, team_id, created_at, updated_at) values
 ('00000000-0000-4000-8000-000000000201', 'Sample Site', 'example.test',      'sample-share', '00000000-0000-4000-8000-000000000101', '00000000-0000-4000-8000-000000000101', null, '2024-01-06 08:00:00', '2024-01-06 08:00:00'),
 ('00000000-0000-4000-8000-000000000202', 'Team Site',   'team.example.test', null,           null, '00000000-0000-4000-8000-000000000102', '00000000-0000-4000-8000-000000000110', '2024-01-06 09:00:00', '2024-01-06 09:00:00');

insert into session (session_id, website_id, browser, os, device, screen, language, country, region, city, distinct_id, created_at) values
 ('00000000-0000-4000-8000-000000000301', '00000000-0000-4000-8000-000000000201', 'chrome',  'Windows 10', 'desktop', '1920x1080', 'en-US', 'US', 'US-CA', 'Sampleville', 'sample-user-1', '2024-02-01 10:00:00'),
 ('00000000-0000-4000-8000-000000000302', '00000000-0000-4000-8000-000000000202', 'firefox', 'Linux',      'laptop',  '1366x768',  'de-AT', 'AT', 'AT-9',  'Wien',        null,            '2024-02-01 11:00:00');

insert into website_event (event_id, website_id, session_id, visit_id, created_at, url_path, url_query, referrer_path, referrer_domain, page_title, event_type, event_name, hostname, utm_source, utm_medium, utm_campaign) values
 ('00000000-0000-4000-8000-000000000401', '00000000-0000-4000-8000-000000000201', '00000000-0000-4000-8000-000000000301', '00000000-0000-4000-8000-000000000a01', '2024-02-01 10:00:00', '/',        '',         '/search', 'search.test', 'Home',    1, null,       'example.test', 'newsletter', 'email', 'spring'),
 ('00000000-0000-4000-8000-000000000402', '00000000-0000-4000-8000-000000000201', '00000000-0000-4000-8000-000000000301', '00000000-0000-4000-8000-000000000a01', '2024-02-01 10:01:00', '/pricing', null,       null,      null,          'Pricing', 2, 'purchase', 'example.test', null, null, null),
 ('00000000-0000-4000-8000-000000000403', '00000000-0000-4000-8000-000000000202', '00000000-0000-4000-8000-000000000302', '00000000-0000-4000-8000-000000000a02', '2024-02-01 11:00:00', '/docs',    'q=sample', null,      null,          'Docs',    1, null,       'team.example.test', null, null, null);

insert into event_data (event_data_id, website_id, website_event_id, data_key, string_value, number_value, date_value, data_type, created_at) values
 ('00000000-0000-4000-8000-000000000501', '00000000-0000-4000-8000-000000000201', '00000000-0000-4000-8000-000000000402', 'plan',     'pro', null,    null, 1, '2024-02-01 10:01:00'),
 ('00000000-0000-4000-8000-000000000502', '00000000-0000-4000-8000-000000000201', '00000000-0000-4000-8000-000000000402', 'revenue',  null,  49.9000, null, 2, '2024-02-01 10:01:00'),
 ('00000000-0000-4000-8000-000000000503', '00000000-0000-4000-8000-000000000201', '00000000-0000-4000-8000-000000000402', 'currency', 'EUR', null,    null, 1, '2024-02-01 10:01:00');
insert into session_data (session_data_id, website_id, session_id, data_key, string_value, number_value, date_value, data_type, distinct_id, created_at) values
 ('00000000-0000-4000-8000-000000000511', '00000000-0000-4000-8000-000000000201', '00000000-0000-4000-8000-000000000301', 'plan', 'pro', null, null, 1, 'sample-user-1', '2024-02-01 10:00:00');

insert into revenue (revenue_id, website_id, session_id, event_id, event_name, currency, revenue, created_at) values
 ('00000000-0000-4000-8000-000000000521', '00000000-0000-4000-8000-000000000201', '00000000-0000-4000-8000-000000000301', '00000000-0000-4000-8000-000000000402', 'purchase', 'EUR', 49.9000, '2024-02-01 10:01:00');

insert into report (report_id, user_id, website_id, type, name, description, parameters, created_at, updated_at) values
 ('00000000-0000-4000-8000-000000000601', '00000000-0000-4000-8000-000000000101', '00000000-0000-4000-8000-000000000201', 'funnel', 'Sample funnel', 'Home to pricing', '{"window": 60, "steps": [{"type": "url", "value": "/"}, {"type": "url", "value": "/pricing"}]}', '2024-02-02 08:00:00', '2024-02-02 08:00:00');
insert into segment (segment_id, website_id, type, name, parameters, created_at, updated_at) values
 ('00000000-0000-4000-8000-000000000611', '00000000-0000-4000-8000-000000000201', 'segment', 'Desktop', '{"filters": [{"name": "device", "operator": "eq", "value": "desktop"}]}', '2024-02-02 08:00:00', '2024-02-02 08:00:00');

commit;
