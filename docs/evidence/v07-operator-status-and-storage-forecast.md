# v07 evidence: operator status and storage forecast

Status: in progress. Every work item is built. The 124M-record sampling run time and the v02 summary counters are still open (see Open items).

## Acceptance criteria

| Criterion | Result | Evidence |
| --- | --- | --- |
| Forecast outcome and date match hand-computed values at a pinned clock | Passed | `maintenance::samples::tests`: young (10 days, not enough history), steady (levels off near 42,188,512,896 bytes on 2026-11-20), reserve (reaches the reserve on 2026-11-09). Arithmetic is in the test comments. `tests/operator.rs` checks the same through `operator.status` with fixture rows and `Clock::frozen`. |
| Non-admins are refused; card and page never render for them | Passed | `operator_status_is_for_install_admins_only` (workspace owner, member, viewer 403; signed out 401) and 12 rows in `fixtures/access/cases.json` (AX-G1122 to AX-G1133). E2E: the viewer gets the not-found page at `/admin/status` and no `operator.status` request is sent. `ServerCard` renders nothing unless `install_role` is `install_admin` and never enables the query otherwise. |
| The page names every readiness problem in words | Passed | `health_tells_disk_full_...` now asserts that `operator.status` names the same problems as `/health/ready` for each health fixture. `src/lib/operator-status.test.ts` checks a distinct title and description for all six in English and German. Screenshot of `disk_full` below. |
| Sampling reads file sizes and `day_aggregates` only | Passed | `samples::take` reads file metadata and two `day_aggregates` queries; `a_sample_takes_retained_records_and_median_ingest_from_day_aggregates`. |
| Sampling run time on the 124M-record fixture | Not run | No full-scale gate directory exists on this machine, and building one (`holocron-gate load --groups-per-month 1000000`, about 13 GB) was out of scope for this task. |
| No event bodies, identity inputs or site data in the result | Passed | `the_forecast_comes_from_the_latest_sample_and_names_no_site` checks that no site ID, workspace ID or domain appears. The result holds counts, sizes, times, states and fixed codes only. |

## Commands

```
cargo fmt --all --check
cargo clippy --workspace --all-targets
cargo test -p holocron-server
cargo run -p holocron-protocol --bin export-types -- --check
cd apps/web && bun run typecheck && bun run lint && bun run test
```

E2E and screenshots against a debug server with `seed_demo` plus `seed_traffic 45`:

```
holocron init --data-dir DATA --admin-email mike@example.test --admin-password-file PW
cargo run -p holocron-server --example seed_demo -- DATA mike@example.test VIEWER_PASSWORD
cargo run -p holocron-server --example seed_traffic -- DATA 45
HOLOCRON_WEB_DIR=apps/web/dist holocron serve --data-dir DATA --bind 127.0.0.1:8197
cd apps/web && HOLOCRON_E2E_URL=http://127.0.0.1:8197 HOLOCRON_E2E_PASSWORD=… HOLOCRON_E2E_VIEWER_PASSWORD=… \
  HOLOCRON_E2E_SHOTS=../../docs/evidence/v07 bunx playwright test e2e/operator-status.spec.ts
```

The disk-full screenshots come from the same data served with `HOLOCRON_DISK_RESERVE_MIB=200000`.

## Screenshots

`v07/admin-status-{1440,390}-{light,dark}.png` (healthy, levels-off forecast with inputs open) and `v07/admin-status-disk-full-1440-{light,dark}.png` (disk full, reaches-reserve forecast).

## Open items

- v02 summary-step counters (runs, skips, timeouts) are not in `OperatorStatus` until v02 lands.
- Compaction keeps no `maintenance_jobs` row, so its outcome is the last pass since start, held in memory.
- v03 mounts `ServerCard` in the sidebar.
