# Milestone 09 evidence: replay and heatmaps

Status on 2026-10-04: built and tested locally, with three open items. Update 2026-10-05: restricted-share filtering is done with milestone 10, two items remain. Recording works end to end with the unchanged Umami 3.4.0 recorder: policy, masking, exclusions, cap, retention, playback in an isolated frame, heatmaps and the Kit screens. The open items:

- Backup and restore of `recordings.sqlite` waits for milestone 12.
- Capacity numbers come from a debug build on a laptop, not from the reference host.

Decisions are in [0011](../decisions/0011-replay-store-and-isolation.md). Mike confirmed its two Proposed items on 2026-10-04: the frame keeps the dashboard's origin on a single host name, and heatmap days are UTC.

## Run

- Source: commit `a02861b` plus the uncommitted milestone 02 to 05 changes in this worktree, plus this milestone.
- Recorder: `sources/umami-3.4.0/recorder.js`, sha256 `21e6312866db01a71841b772c9882333231be1bc3fdbeeac1ed5193794ad80c6`, unchanged. Player: rrweb 2.0.1, pinned to the recorder's version.
- Host: Apple M1 Pro, 10 cores, 32 GiB, macOS (Darwin 27.0.0), Chromium from Playwright. No reference host.

Commands, from the repository root:

```sh
cargo test -p holocron-server -p holocron-protocol      # 74 server tests, 9 of them in tests/recordings.rs, plus drift
cargo clippy -p holocron-server -p holocron-protocol --all-targets
cargo run -p holocron-protocol --bin export-types -- --check
(cd apps/web && bun run typecheck && bun run lint && bun run test && bun run build)

# browser: packaged server on seed_demo data
holocron init --data-dir D --admin-email mike@example.test --admin-password-file PW
cargo run -p holocron-server --example seed_demo -- D mike@example.test 'VIEWER PW'
HOLOCRON_DISK_RESERVE_MIB=256 holocron serve --data-dir D --bind 127.0.0.1:8099 --web-dir apps/web/dist
(cd apps/web && HOLOCRON_E2E_URL=http://127.0.0.1:8099 HOLOCRON_E2E_PASSWORD=... \
  HOLOCRON_E2E_VIEWER_PASSWORD=... HOLOCRON_E2E_SHOTS=../../docs/evidence/m09 \
  npx playwright test e2e/recordings.spec.ts e2e/shell.spec.ts)

# recorder compatibility: the m01 harness against Holocron
sqlite3 D2/control.sqlite < tools/collection-harness/seed-holocron.sql
UMAMI_URL=http://127.0.0.1:33400 node tools/collection-harness/capture.mjs --out docs/evidence/m09/browser-captures

# maintenance at scale (debug build, see Capacity)
cargo run -p holocron-server --example recording_maintenance -- SCRATCH 2000 100000
```

## What was built

| Work item | Where | Behaviour |
| --- | --- | --- |
| 1 handshake, wire | `src/recordings/wire.rs`, `src/collect/record.rs` | `GET /api/websites/{id}/recorder` and `POST /api/record` as upstream, with the session cache token, envelope limits and heatmap number coercion from upstream |
| 1, 3 parts, order, duplicates | `src/recordings/store.rs`, `replay.rs` | Capture = `(site, visitId)`. Parts are ordered by recorder index, then arrival. The same bytes at the same index are stored once. Split events are restored in any arrival order, and a missing fragment counts as a gap |
| 2 policy | `src/recordings/policy.rs`, `recordings.policy` / `updatePolicy` | Off by default, strict masking always, 10 % sampling default, `blockSelector` from element exclusions, path patterns for page exclusions |
| 3 identity | `recordings.list`, `recordings.get` | Captures store the daily visitor pseudonym, never a session ID. The analytical session is found at read time |
| 4 cap, retention | `store.rs`, `maintenance.rs` | Cap of 25 GiB compressed, checked in the same commit as the part. The saved sub-cap is one fifth. Removal is in 32-capture steps at site-local `today − 14`, followed by incremental vacuum. Playback pages are bounded |
| 4 heatmaps | `src/recordings/heatmaps.rs` | Interactions go through the inbox into DuckDB. After an idle hour they fold into per-day cells: 3 viewport classes, 64 columns, 32 px rows, scroll deciles. 90-day removal |
| 5 screens | `apps/web/src/pages/{Replays,Replay,Heatmaps,RecordingSettings}Page.vue`, `pages/recordings/` | Filters live in the URL. The player has play, seek, speed and skip-inactivity. The event timeline is a keyboard-operable list with `aria-current`. Heatmaps show a click-density outline, a top-areas table and a scroll-depth table. Policy form and budget card. Kit switch, textarea, progress and tabs were copied in |
| 6 isolation | `replay-frame.html`, `src/replay-frame/`, `src/recordings/frame.rs` | A separate page with a no-network CSP and a sandbox. rrweb's iframe runs no scripts and gets no pointer input. Active content is stripped on the server. Messages go through `postMessage` with sender and shape checks |
| 7 workloads | `examples/recording_maintenance.rs`, `e2e/recordings.spec.ts` | Measures retention and folding at scale. Browser workload for ingestion, playback, hostile content and permissions |

## Acceptance criteria

| Criterion | Result | Evidence |
| --- | --- | --- |
| Unchanged recorder passes capture and playback fixtures | Pass. 24 of 24 harness scenarios against Holocron, including the new `recorder-element-exclusion`. Playback of a real recorder capture in Chromium | `browser-captures/`, e2e "a replay plays…" |
| Default disabled transmits and retains nothing; masking and exclusions verified in the browser before transmission | Pass. `recorder-disabled-site` sends no record. The e2e inspects every `/api/record` body the recorder sent: no page text, no blocked element, no typed input | `recording_is_off_until_enabled…`, e2e "the unchanged recorder masks…" |
| Cap refuses replay while events still collect | Pass. Over the cap, a part gets `{ok:false, reason:"replay_budget_exceeded"}` and the capture is marked truncated. Events and heatmaps are still accepted. The budget card shows the refusal | `a_full_cap_refuses_replay_while_events_and_heatmaps_continue` |
| Reordering, duplication and fragment recovery do not corrupt playback | Pass. Parts were sent out of order, duplicated and split, and play back once in recording order | `reordered_duplicated_and_split_parts…`, `fragments_restore_in_any_order…` |
| Session corrections keep replay order | Pass. A correction moves the session link, and the replay itself is unchanged | `session_corrections_leave_the_replay_intact…` |
| Retention, cap cleanup and folding stay bounded | Pass locally (debug build): slowest retention step 25 ms, slowest fold step 392 ms | Capacity |
| Reads enforce SiteScope and restricted sharing | SiteScope: pass. Every `recordings.list` and `recordings.get` case of the access fixture matches. A member of another workspace, a removed member and a signed-out user are refused, and a viewer can read replays but cannot configure recording. Restricted sharing: pass since 2026-10-05. Reads keep to a share's range and pinned filter, and need the recordings grant | `replay_reads_follow_the_access_fixture`, `share_viewers_reach_only_replays_inside_the_share`, share rows in `tests/access.rs`, e2e "viewers…" and the share viewer recordings case ([m10](m10-access-and-sharing.md)) |

## Hostile content

Ten cases in [`fixtures/replay-hostile/`](../../fixtures/replay-hostile/): script elements, event handlers, script URLs, framed documents, SVG script, form submission, meta refresh and base, remote resources, remote stylesheets and mutation-injected scripts. Three layers keep them inert:

1. The server strips active parts from every playback page (`hostile_fixtures_lose_every_active_part`).
2. rrweb renders into an iframe without `allow-scripts`.
3. The frame's CSP has `connect-src`, `frame-src` and `form-action` set to `'none'` and allows images only from `data:` and `blob:`.

In Chromium, all ten cases were sent through `/api/record` and played back. Across all ten, a route on the beacon host was reached 0 times, and no dialog opened. The dashboard title was never changed. The rendered DOM contained no `<script`, `onerror=`, `javascript:` or `srcdoc=`. Before stripping was involved, Playwright reported each remote image and stylesheet as blocked by `csp` without a request leaving the browser.

The frame shares the dashboard's origin, because rrweb cannot render inside an opaque-origin sandbox ([0011](../decisions/0011-replay-store-and-isolation.md) Evidence). That item is Proposed.

## Capacity

Debug build (`cargo build`, unoptimized). The release build failed for lack of disk space and was not retried. One run on the laptop, scratch data on APFS:

| Measurement | Result |
| --- | --- |
| Replay write, 2,000 captures × 5 parts of 64 KiB rrweb-shaped JSON | 624 MiB raw → 36.0 MiB stored (zstd 3, 17×), 55.8 MiB file, 156 parts/s |
| Retention of all 2,000 captures | 63 steps of 32 captures, slowest 25.0 ms, total 0.46 s |
| Incremental vacuum | 2 steps, 0.13 s; file 55.8 → 23.1 MiB |
| Heatmaps, 100,000 interactions over 90 days | admitted and applied in 2.02 s |
| 90-day cell read before folding | 44.1 ms |
| Fold | 2 steps of 10,000 captures, slowest 391.5 ms, total 0.40 s, 23,340 cells |
| 90-day cell read after folding | 21.6 ms, same click total |

The 156 parts/s write rate is far above the recorder's rate per visitor, which is one part every few seconds. It is not a burst measurement with concurrent events. That belongs in the milestone 13 capacity run.

## Browser evidence

`e2e/recordings.spec.ts`, 10 of 10 pass. `e2e/shell.spec.ts` still passes (9 pass, 1 skipped). Screenshots are in [`m09/`](m09/): replays, replay, heatmaps (clicks and scroll) and recording settings, at 1440 and 390 px, light and dark. None of the screens overflows horizontally at 390 px.

After the merge with the milestone 05 settings screens (2026-10-04), recording settings are the fourth section of the website settings page at the same URL, `/sites/:siteId/settings/recording`, and the separate link card on the general section is gone. On the merged tree both suites pass on a fresh `seed_demo` (debug server): recordings 10 of 10, shell 11 passed and 1 skipped. One earlier run timed out in "a replay plays…" on the last Pause click. A short replay at 4× can end before that click, so the step is timing-dependent.

## Deviations from upstream

- Masking is always strict and the default sampling is 10 % (upstream: moderate masking, 15 %). This is already recorded in the capability matrix.
- `POST /api/record` can answer `{ok:false, reason:"replay_budget_exceeded"}` or `"page_excluded"`. The unchanged recorder ignores the body, so nothing breaks.
- The unchanged recorder cannot skip a page. Holocron drops data from excluded pages after the browser has sent it. Product documentation must say this.
- Server settings do not prove that a client masked its payload. Only the recorder this server serves masks as described. The settings screen says so.

## Not verified

- Release-build and reference-host numbers, and recording under the 500 events/s burst. These belong to milestone 13.
- Backup and restore of `recordings.sqlite`. This belongs to milestone 12.
- Import mapping of upstream `session_replay` and `heatmap_event`. Keys are defined in 0011, and the work belongs to milestone 11.
- Browsers other than Chromium.

## Handoff

- 10: filter replay reads by a share's range and pinned filter.
- 11: map upstream recordings by site, visit ID and chunk index (0011 Capture identity).
- 12: snapshot `recordings.sqlite`, delete captures with their site, and read the budget into operations status.
- 13: run `recording_maintenance` in release on the reference host, and add recording to the burst test.
