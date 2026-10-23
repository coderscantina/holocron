# Milestone 05 evidence: identity, sessions, and corrections

Status on 2026-10-04: in progress. Sessions, corrections, policy versions and freshness are built on the milestone 02 storage layer and tested through real admission, transfer and DuckDB. The hand-audited fixture matches every expected session and every traffic case. Since 2026-10-04 the milestone 04 server is integrated: collection stamps records with the one identity-day implementation, consent grant and revocation work over HTTP, the session and identity policy, recalculation and freshness operations exist, and correction runs in the server lifecycle. The milestone cannot close yet: the fixture's other report families need the query compilers of milestones 06 to 08, and the correction workload has only run at 1% scale on a laptop. The settings screens for sessions and identity are built since 2026-10-04.

## Run

- Source: commit `a02861b` plus the uncommitted milestone 02, 03 and 04 changes in this worktree, plus this milestone.
- Engines: DuckDB 1.5.6 (duckdb-rs 1.10506.0, bundled), SQLite 3.51.3 through SQLx 0.9.0.
- Host: Apple M1 Pro, 10 cores, 32 GiB, macOS (Darwin 27.0.0), APFS SSD. No container, no reference host.

Commands, from the repository root:

```sh
cargo test -p holocron-analytics                                    # 6 unit, 18 behaviour, 2 fixture tests
cargo test -p holocron-analytics --features failpoints --test crash # 20 process kills inside correction
cargo test -p holocron-storage                                      # 5 storage tests
cargo test -p holocron-server --test identity                       # 6 HTTP tests of this milestone
cargo test -p holocron-server                                       # every server test, 53 in total
cargo test -p holocron-protocol                                     # includes the generated-contract drift check
cargo clippy -p holocron-analytics --all-targets --features failpoints
cargo clippy -p holocron-analytics -p holocron-protocol -p holocron-server --all-targets
cargo build --release -p holocron-workload
B=target/release/holocron-gate
HOLOCRON_DISK_RESERVE_MIB=256 $B load --data DATA --groups-per-month 10000
HOLOCRON_DISK_RESERVE_MIB=256 $B correct --data DATA --out OUT --groups-per-month 10000 --late-spacing-ms 100
```

`HOLOCRON_DISK_RESERVE_MIB` lowers the 4 GiB admission disk reserve ([0008](../decisions/0008-storage-engine-and-budgets.md)) because the volume had 1 to 2 GB free. It is a measurement override, not a budget change.

## What was built

| Work item | Where | Behaviour |
| --- | --- | --- |
| 1, 2 identity semantics | `crates/analytics/src/identity.rs` | Identity day and timezone epoch from the session policy versions, end of an identity day (next local midnight or the next epoch's effective instant), day key destruction instant `end(D) + 25h`, consent links by event time, session populations `Daily` and `Profile`. Collection calls these; day keys and keyed hashing stay in collection (milestone 04) |
| 2 revocation | `Record::Revocation`, `consent_revocations` | A revocation travels through the inbox like any record and closes open profile sessions at its instant |
| 3 sessions | `crates/analytics/src/correction.rs` | Per identity, order eligible events by `(event_time, seq)`; a gap of at least the window starts a session; daily sessions end at an identity-day change; profile sessions end at a revocation. Performance and identify rows attach without changing boundaries |
| 4 correction | same | One step takes the oldest dirty identity days (64 by default), rebuilds every derived row they can affect, refreshes their day aggregates and clears their dirty rows in one transaction |
| 4 versioned aggregates | `day_aggregates` | Per site and identity day: event counts, distinct daily pseudonyms, sessions, bounces, span sum, policy versions, `metric_version`, and the batch they were computed at. The exact identity-day rollup input [0008](../decisions/0008-storage-engine-and-budgets.md) proposes for milestone 06 |
| 5 freshness | `crates/analytics/src/freshness.rs` | Per result: ingestion watermark, `corrected_through` batch, `Corrected` or `Pending { dirty_days }`, `metric_version`, session policy versions in range |
| 6 jobs | `crates/analytics/src/jobs.rs` | `create_site`, `change_session_policy` (timezone changes only forward in time; a window change effective in the past is an explicit recalculation), `recalculate` over a date range |
| reference read | `crates/analytics/src/traffic.rs` | Traffic summary over derived sessions for the fixture and tests. Milestone 06's planner owns grouped and filtered explorations |
| 1, 2 collection identity | `apps/server/src/pseudonyms.rs`, `collect/mod.rs` | Identity day, timezone epoch and key destruction instant from `SitePolicies`; day keys per `(source, epoch, local day)`; retention cutoff from the timezone in effect at receive time. `calendar.rs` is gone |
| 2 consent | `apps/server/src/consents.rs`, `collect/mod.rs`, `collect/payload.rs` | The `consent` key parsed as `persistent`, `none` or unrecognized. Honored with persistent identity on and an `id`; otherwise counted in `consent_ignored`. Grant opens an interval, `none` closes it and sends a revocation record with the request's items. `Event.profile_id` and `Identify.profile_id` are set from `Pseudonyms::profile_id` when consent is active at event time |
| 5, 6 operations | `apps/server/src/sites/mod.rs`, `sites/analytics.rs`, `crates/protocol/src/operations/` | `sites.updateSessionPolicy`, `sites.identityPolicy`, `sites.updateIdentityPolicy`, `recalculations.start`, `recalculations.status`, `analytics.freshness` |
| policy ownership | `apps/server/src/sites/policy.rs`, `jobs::record_policies` | Control storage owns the session policy versions; one mirror function writes the analytical copy, before a policy command commits and for every site at startup ([0002](../decisions/0002-sessions-and-events.md) Policy ownership) |
| lifecycle | `apps/server/src/background.rs` | `correction::follow` and day key destruction start with the server and stop on graceful shutdown before analytical storage closes; a running correction step commits first |

A dirty day affects the daily sessions of that day, rebuilt from that day's events alone, and every session of each profile with an event or identify on that day, rebuilt from the profile's whole history because profile sessions cross midnight. Rebuilding from immutable events makes a rerun produce the same rows.

## Server integration

Design choice for the split source of truth: control storage owns every policy fact, as 0008 assigns sites and settings to `control.sqlite`. Migration `0003_identity.sql` moves the timezone and epoch from `sites` into `session_policies` (one row per version, backfilled as version 1 with the default 30-minute window) and drops `sites.timezone` and `sites.timezone_epoch`. DuckDB `session_policies` is a derived copy. `jobs::create_site` and `jobs::change_session_policy` are replaced by `jobs::record_policies`, which makes the copy equal to the owner and marks the affected days dirty. Validation stays in `SitePolicies::change`, which the server runs against the owner's versions. A separate create and change path would have had to re-validate on replay, and a timezone change replayed after a restart would fail the "not in the past" rule.

`crates/analytics/src/identity.rs` is the one identity-day implementation: `apps/server/src/calendar.rs` is deleted, and its retention cutoff and event-time bounds moved to `SitePolicies::retention_cutoff_ms`, `MIN_TIME_MS` and `MAX_TIME_MS`. A side effect: the key destruction instant of the last day before a timezone change is now the change instant plus 25 hours, as 0001 says. The old calendar code used the next local midnight.

`apps/server/tests/identity.rs`, each through the real router, inbox, transfer and DuckDB:

| Test | Shows |
| --- | --- |
| `tracker_payloads_become_corrected_sessions_and_freshness_says_when` | two `/api/send` pageviews: `analytics.freshness` is `pending` with 1 dirty day; the started background corrects them into one session; freshness turns `corrected` with `corrected_through` equal to the watermark batch, versions [1], metric version 1. A late event 15 minutes earlier extends the session backwards to 3 pageviews |
| `consent_links_events_until_it_is_revoked` | `consent` before the site enables persistent identity links nothing; after enabling, a grant, a linked event, `"none"` and an unlinked event; the revocation record carries the linked profile; the consent interval is `[grant, revoke)` |
| `turning_persistent_identity_off_revokes_every_profile` | viewers get `forbidden`; on then off bumps the identity policy version to 3, closes every open interval, stores one site-wide revocation record, and later events stay unlinked |
| `session_policy_versions_reach_collection_and_analytics` | viewers get `forbidden`; a retroactive timezone change is `invalid_arguments` on `effective_at`; a window change and a scheduled timezone change make versions 2 and 3; the DuckDB copy equals them; events either side of the change take day keys of epochs 1 and 2 |
| `recalculation_reports_its_status` | a backwards range is refused; a two-day recalculation is `pending` with 2 days, then `corrected` once the background corrects it |
| `startup_repairs_the_analytical_policy_copy` | a version committed only in control storage reaches the copy through the startup repair |

`crates/analytics/tests/sessions.rs` adds `recorded_policies_follow_their_owner_and_rebuild_what_changed`: recording the same versions again changes nothing, a window change rebuilds sessions as version 2, and a rollback of the owner drops version 2 from the copy and rebuilds again.

Authorization: every new command and the two settings queries need `site.configure` through the same `site_scope` resolution as `sites.update`, so hidden sites answer `not_found` and members without the capability `forbidden`. `analytics.freshness` needs `site.read`.

## Fixture results

`tests/correctness_fixture.rs` plays collection's part for the 166 input records with the 0001 rules from `identity.rs`: identity day and epoch, keyless arrival, the retention cutoff, consent intervals by event time, and revocation records. Pseudonyms are stand-in hashes of the fixture's identity labels, so results name identities exactly as the fixture does. Admission returns 2 duplicates and the harness suppresses `ret-01`, as `outcomes.json` states.

| File | Expected | Actual |
| --- | --- | --- |
| `sessions.json` sessions | 78, each with identity, members, start, span, pageviews, custom events, bounce, policy | 78 equal |
| `sessions.json` performance attachment | `perf-07` in {gap-02, gap-03}, `perf-01` to `perf-06` unattached | equal |
| `sessions.json` keyless events | key-01, key-02, ret-02 | equal |
| `traffic.json` site-a-all | 84 pageviews, 54 events, 68 sessions, 61 visitor-days, 4 people (11 / 138), bounce 24 / 60, span sum 13,304,998, mean 221,750, median 60,000, coverage 135 / 138, 34-day series | every field equal |
| `traffic.json` site-b-all | 9, 2, 9 sessions, 8 visitor-days, series sums to 9 with `series_sum_differs`, versions [1, 2], bounce 7 / 8, mean 67,500, median 0 | every field equal |
| `traffic.json` dst-fall-2025, dst-spring-2026 | 4 pageviews, 3 sessions, 2 visitor-days, spans 1,500,000 and 1,200,000 | every field equal |
| `traffic.json` day-2026-06-02 | label "visitors" 4, 7 pageviews, 4 events, 5 sessions, coverage 9 / 11, mean 454,000, median 5,000 | every field equal |
| `traffic.json` day-2026-06-03 | 1 session of 1,499,000 ms from two day-late events | every field equal |
| `traffic.json` identified-2026-09-14-to-27 | 3 people, 7 / 9 linked, 7 visitor-days | every field equal |

Not compared here: `realtime.json`, `goals.json`, `funnels.json`, `journeys.json`, `attribution.json`, `retention.json`, `revenue.json`, `percentiles.json`, `properties.json` and `acceptance-exploration.json`. Their formulas need the query compilers of milestones 06 to 08. They read the derived sessions, members and populations built here. Two collection-surface fields stay with milestone 04: revenue classification and typed properties.

## Behaviour tests

`crates/analytics/tests/sessions.rs`, each through admission, transfer and correction:

| Test | Shows |
| --- | --- |
| `late_event_extends_a_session` | an event received the next day joins its session; span grows to 35 minutes |
| `late_event_joins_two_sessions` | a late event inside a 40-minute gap merges two sessions |
| `late_revocation_splits_a_profile_session` | a revocation received after both events splits the profile session |
| `window_change_splits_sessions_from_its_effective_instant` | 30 to 15 minutes from 09:00: later sessions split and carry version 2, earlier ones keep version 1 |
| `anonymous_sessions_close_at_local_midnight_and_profiles_cross_it` | 23:50 and 00:05 local: two daily sessions, one profile session; a pseudonym value that repeats across the day boundary still splits |
| `gaps_are_measured_in_instants_across_dst` | both Vienna transitions: 25 and 20 minute instant gaps extend; the 25-hour and 23-hour days split at local midnight |
| `inactivity_window_is_half_open_and_one_event_sessions_have_zero_span` | exactly 30 minutes splits, 30 minutes minus 1 ms extends; a one-event session has span 0 |
| `timezone_change_rotates_identity_and_versions_sessions` | New York to UTC at 12:00Z: anonymous inputs 20 minutes apart give two sessions (versions 1 and 2), the profile session continues; freshness lists versions [1, 2] |
| `identified_mode_transitions_never_merge_history` | anonymous, then `identify` without consent, then linked, then unlinked: three sessions, 2 visitors, 1 person |
| `consent_revocation_keeps_history_and_closes_the_profile_session` | linked event, revocation, unlinked event, re-grant within the window: two profile sessions and one daily session; the person still counts with 2 linked events |
| `performance_and_identify_attach_without_moving_boundaries` | performance inside `[start, last + window)` attaches, after it does not; an identify before any session attaches to the next session that day |
| `keyless_events_count_without_forming_sessions` | keyless events count in pageviews and events, not in sessions or visitors; coverage 1 / 3 |
| `recalculation_leaves_capture_ids_and_chunk_order_intact` | replay fragments and heatmap rows keep sequence, capture ID, chunk order and payload hash through a merge and a full-day recalculation |
| `correction_is_idempotent_and_replay_is_deterministic` | records admitted in seven-record waves with correction after each equal one admission and one correction, row for row; recalculating everything again changes nothing |
| `reports_stay_pending_while_their_days_are_dirty` | a range touching a dirty day is `Pending`, another range is `Corrected`; after correction `corrected_through` equals the watermark batch |
| `day_aggregates_match_the_row_level_summary` | every aggregate column equals the traffic summary for its day |
| `correction_steps_are_bounded` | 5 dirty days with a 2-day limit take 3 steps: (2, 3 left), (2, 1), (1, 0) |

Unit tests in `identity.rs` cover the end of the last old-epoch day at a timezone change, DST day lengths, refusal of a retroactive timezone change and out-of-order versions, consent intervals, and the retention cutoff at a month end.

## Privacy-policy cases

- **No cross-day anonymous profile.** A daily pseudonym is keyed per identity day by collection. Derivation also ends a daily session at any identity-day change, so even a repeated pseudonym value is never joined across days (test above). Report reads return counts; visitor-days are distinct daily pseudonyms per day, never deduplicated across days. The only person-level key is the profile ID, and only linked events carry one.
- **Keyless events** count exactly in pageviews, events and revenue. They are in no session and lower identity coverage (fixture: 135 / 138).
- **Consent absence.** Site B ignores `consent` because persistent identity is disabled; eve's events on two devices form one daily identified session and 0 people (fixture).
- **Revocation.** Linked facts stay attributed: dora's linked event still counts as a person and her same-day unlinked event shares her one visitor-day (fixture and test). The revocation closes the profile session at its instant, also when a re-grant follows within the window (test).
- **Grant.** No backfill: events before the grant stay in their daily session (fixture con-05, anna's cohort starts at con-07).

## Policy-version transitions

| Transition | Rule | Evidence |
| --- | --- | --- |
| Site created after events arrived | Version 1 applies to every event time; existing days are rebuilt | `create_site` marks the site's days dirty |
| Timezone change | New version and epoch at an instant not before now; anonymous sessions close there, profile sessions continue; results spanning it list both versions | timezone test, fixture site B |
| Retroactive timezone change | Refused: old day keys may be gone | unit test |
| Window change | New version; a gap is judged by the window in effect at its earlier event; a session carries the version at its first event; an instant in the past is an explicit recalculation from there | window test |
| Recalculation | `recalculate(site, days)` marks days dirty; correction rebuilds them under the current versions | capture and determinism tests |

## Correction crash cases

`tests/crash.rs` runs `analytics-crash-child` 20 times with fixed seeds. Each run admits 180 of 240 seeded records on two sites over eight days, with consented profiles crossing midnight. It derives everything in one write and saves that state. It then admits the last 60 records as late events for corrected days and corrects in 3-day steps. `HOLOCRON_FAILPOINT=write.before_commit:n` sends SIGKILL inside the n-th correction transaction, after all its writes and before commit. A second process reopens the store and checks, per identity day and per profile, that the state is either the saved one or the reference built from all records in a fresh store. Days that differ from the reference must still be dirty, so freshness labels them pending. It then finishes correction and requires the reference exactly.

| Kill | Seeds | Days replaced before the kill | Result |
| --- | --- | --- | --- |
| hit 1, first derivation | 0, 5, 10, 15 | 0 | 4 of 4 recover |
| hit 2, first replacing step | 1, 6, 11, 16 | 0 | 4 of 4 |
| hit 3 | 2, 7, 12, 17 | 3 | 4 of 4 |
| hit 4 | 3, 8, 13, 18 | 6 to 8 | 4 of 4 |
| hit 5 | 4, 9, 14, 19 | 9 to 10 | 4 of 4 |

macOS only. The milestone 02 table row "During correction application" is this boundary.

## Resource traces

Local, release build, 1% scale. Not a capacity claim. Run 2 below is the measured run; run 1 is kept as a failed run.

- Load: 10,000 groups per month, 1,240,008 records through admission, inbox and transfer in 31 s; `analytics.duckdb` 164 MB after correction.
- Backfill of every dirty day: 126,860 sessions in 16.8 s, in 3 steps of up to 64 days: 0.45 s, 7.7 s and 8.6 s. Resident memory 107 MB before, 505 MB after, 595 MB peak.
- Correction load: 1,000 late events (manifest kinds and offsets around 8,771 target groups), 200 keyless events (manifest targets) and 100 identify changes, sent over 100 s instead of the manifest's 30 minutes. 0 refused.
- Follower correcting after every watermark change: 369 steps, p50 273 ms, p95 379 ms, p99 526 ms, max 788 ms. That is how long a step holds the analytical writer and delays transfer.
- Freshness, every record accepted:

| Stream | n | Acknowledgement to visible p50 / p95 / p99 / max (ms) | Acknowledgement to corrected p50 / p95 / p99 / max (ms) |
| --- | --- | --- | --- |
| late | 1,000 | 442 / 708 / 865 / 1,120 | 711 / 1,030 / 1,236 / 1,489 |
| keyless | 200 | 433 / 711 / 826 / 980 | 727 / 1,030 / 1,223 / 1,395 |
| identify | 100 | 414 / 660 / 801 / 821 | 735 / 1,023 / 1,132 / 1,216 |

"Visible" means the watermark covers the record; "corrected" means its identity day has no dirty row from its batch or earlier, the condition under which a result stops being labelled pending. The probe reads every 100 ms, so each figure carries up to 100 ms of probe uncertainty.

- Session oracle: for all 5,849 anonymous target identities, the sessions computed in the harness from the generator's events plus the accepted late events (0002 timeout rule) equal the derived sessions, start, last event and pageviews: 0 mismatches.

Run 1 (same commands, 512 MiB reserve) refused 1,001 of 1,300 records as `Unavailable`, because other work on the volume took its free space below the reserve. Its oracle compared against refused events too, so its 565 mismatches were void. The harness now counts refusals and feeds only accepted records to the oracle.

## Settings screens

Built on 2026-10-04: `/sites/:siteId/settings/sessions` and `/sites/:siteId/settings/identity` (CAP-IDENT-02, 03), reached through section tabs on the website settings page.

- Sessions: inactivity window and timezone with the policy version, and a recalculation card that polls `recalculations.status` every 2 s until the range is corrected. Recalculation is hidden without `site.configure`.
- Identity: the persistent identity switch. Turning it off shows the revocation warning and the button reads "Turn off and revoke consents". Roles without `site.configure` get a notice and the query is not sent.
- `RECALCULATION_MAX_DAYS` (366) moved from the server into the generated protocol limits, so the UI hint and the server check share one value.

Two browser journeys in `apps/web/e2e/shell.spec.ts` cover validation, save, recalculation to completion, enable and revoke, and the viewer case. The full shell suite passes: 11 passed, 1 skipped by design. Screenshots: [sessions](m05/settings-sessions-light.png), [identity revoke](m05/settings-identity-revoke-light.png), [viewer](m05/settings-identity-viewer-dark.png).

The journey found two bugs, both fixed:

- **Recalculations never ran on an idle site.** The correction loop woke only when the ingestion watermark moved, so a recalculation or session policy change waited for the next collected event. `follow` now also waits on a `Notify` in `AppState`, which `recalculations.start` and `sites.updateSessionPolicy` notify after marking days dirty. `recalculation_reports_its_status` now starts the loop before the command; it fails without the wake ("still pending" after 10 s) and passes with it.
- **Stale revision after a recalculation.** `recalculations.start` bumps the site revision; the screen now reads the site again afterwards, so the next save does not answer `stale_revision`.

## Not verified

- **Fixture over HTTP.** The correctness fixture still admits `Record`s through a harness that plays collection's part. The HTTP tests above cover the same rules case by case, not the 166 fixture inputs.
- **Trusted server consent path.** Built on 2026-10-06 with site collection keys; a keyed batch grants consent and links the event (`apps/server/tests/collection.rs`). The browser path through `data-before-send` works as before.
- **Fixture report families outside sessions and traffic.** Realtime, goals, funnels, journeys, attribution, retention, revenue, percentiles, properties and the acceptance exploration need milestones 06 to 08. The acceptance criterion "matches all settled formulas" is met for sessions and traffic only.
- **Correction workload at full scale.** Needs the 120M-event store (44 GB), which does not fit on this host now. The manifest's 1,800 ms schedule, revenue deltas, and funnel, attribution and cohort deltas are not measured.
- **Profile rebuild cost at scale.** Each step joins all events against the step's profiles. At 120M events that is a full scan of `site` and `profile_id` per step. Unmeasured.
- **Correction completion target.** Not frozen. The run above gives a first local figure: acknowledgement to corrected p99 1.24 s at 1% scale.
- **Storage failpoint suite after the record and schema changes.** `cargo test --release -p holocron-storage --features failpoints` was not rerun; the debug storage tests pass.
- **Identify changes.** The 100 identify changes attach to sessions; which value wins is a session-property read for milestone 06.
- **Imported population.** `Population` has `Daily` and `Profile`. Imported history needs a provenance field on records from milestone 11 before it can be labelled.
- **Linux, container, reference host.** Not run.

## Interface changes

Since the server integration on 2026-10-04:

- Protocol: commands `sites.updateSessionPolicy`, `sites.updateIdentityPolicy`, `recalculations.start`; queries `sites.identityPolicy`, `recalculations.status`, `analytics.freshness` (new domain file `operations/analytics.rs`); `Site` gains `session_window_minutes` and `session_policy_version`; new ID type `RecalculationId`. TypeScript and OpenAPI regenerated; the drift test passes and `vue-tsc` passes.
- Control migration `0003_identity.sql` (SQLite, server-owned): `session_policies` backfilled from `sites`, `sites.timezone` and `sites.timezone_epoch` dropped, `sites.persistent_identity` and `sites.identity_policy_version` added, `consents`, `recalculations`. Inbox and DuckDB schemas are unchanged, so pending inboxes and backups stay compatible.
- `holocron-analytics`: `jobs::record_policies` replaces `create_site` and `change_session_policy`; `recalculate` returns `Recalculation { days, through_batch }`; `jobs::pending_days`; `SitePolicies::from_versions` is public; `SitePolicies::retention_cutoff_ms`; `MIN_TIME_MS`, `MAX_TIME_MS`; `PolicyError::SiteExists` and `UnknownSite` removed.
- Server: `holocron-server` depends on `holocron-analytics`; `Background::start` and `stop`; `sites::create_site` writes policy version 1 and returns `anyhow::Result`; `AppState::new` repairs every site's policy copy and returns `anyhow::Result`.

Since the settings screens on 2026-10-04:

- `holocron_analytics::follow` takes a `wake: &tokio::sync::Notify`. `AppState` gains `correction: Notify`.
- Protocol limit `RECALCULATION_MAX_DAYS`; TypeScript and OpenAPI regenerated.
- Web: settings routes `site-settings-sessions` and `site-settings-identity` nested under the settings page. Sidebar links now use the active class instead of the exact-active class, so "Website settings" stays highlighted on its sections.

From the analytics work on 2026-10-03:

- `holocron-storage`: `Record::Revocation(Revocation { id, site, profile_id: Option<Id>, revoked_at_ms, receive_time_ms })`, appended as the last variant so earlier encodings still decode. Transfer stores it in `consent_revocations` and marks UTC days −2 to +2 of its instant dirty.
- `ANALYTICS_SCHEMA` 2: `session_policies`, `consent_revocations`, `sessions`, `session_members`, `day_aggregates`. Tables are created on open. The gate's provisional `sessions` table is replaced; the probes read the same column names.
- Failpoint `write.before_commit` inside `Storage::write`.
- `holocron-gate sessions` now runs correction; new `holocron-gate correct`; `HOLOCRON_DISK_RESERVE_MIB` override.

## Open items

- `identity_policy_version` per event (0002 table) is not on `Event`. No acceptance criterion and no derivation needs it yet; it needs an inbox format and DuckDB schema change. Kept per site in control storage.
- Imported population labels need a provenance field from milestone 11.
- Done 2026-10-06: collection site keys for the trusted server consent path.
- Fixture report families outside sessions and traffic: milestones 06 to 08.
- Full-scale correction workload and a frozen correction completion target.
- Correction rebuilds every touched profile's whole history. At full scale a 64-day rebuild step runs out of the 1 GiB budget, and with a year of data in place each live step reruns the session rules over the full year of every active profile. Found on 2026-10-05; proposed fix and stopgap in [m06 time-ordered members and property partitions](m06-member-order-and-property-partitions.md#open-items). Fixed on 2026-10-06 ([0008](../decisions/0008-storage-engine-and-budgets.md#unresolved-risks)); full scale not rerun.
