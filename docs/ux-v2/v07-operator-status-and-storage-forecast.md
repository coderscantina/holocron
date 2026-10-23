# v07: Operator status and storage forecast

## Outcome

Install admins see in the dashboard what today needs a CLI or `/health/ready`: whether collection is healthy, how far analytics lags, disk and recording budgets, recent maintenance outcomes, and a storage forecast that says whether the disk will hold the retained data.

## Dependencies and ownership

Needs nothing from 0015 and can start now. Storage and operations own the status query and size samples; frontend owns the admin page and the sidebar card. The protocol owner merges the operation; the storage owner orders the migration.

Owned: `crates/protocol/src/operations/` new `operator.rs` (or an extension of `collection.rs`, agreed with the protocol owner), `apps/server/src/maintenance/` status assembly and size sampling, a control migration for samples, `apps/web/src/pages/admin/` new status page, the sidebar server card component under `apps/web/src/components/admin/`, tests, and `admin.status.*` locale keys. Read first: [0006](../decisions/0006-retention-clock-and-operations.md) Retention cutoff, [0008](../decisions/0008-storage-engine-and-budgets.md) Budgets and Installation tiers, [0014](../decisions/0014-operations-and-recovery.md) Jobs, Compaction and Health, [design contract](design.md) Server card.

## Current code (2026-10-15)

- `collection.status` (install admins only) returns availability, inbox depth, bytes and lag, watermark, `disk_free_bytes`, `disk_reserve_bytes` and outcome counts. The web app never calls it.
- `/health/ready` names problems: `disk_full`, `storage_unavailable`, `shutting_down`, `transfer_failing`, `analytics_stale`, `recorder_budget`.
- `recordings.budget` returns replay used, cap, saved bytes and file size.
- Retention runs at least hourly; job results stay in `maintenance_jobs` for 30 days without an operation. Compaction runs hourly. Diagnostics and backup are CLI only.
- The admin area has Users and Security pages only.

## Correction to the first concept

The first concept drew a monthly sawtooth with "next retention prune Nov 1". That is wrong for this product: retention removes data at least hourly and queries apply the cutoff as a predicate (0006), so there are no monthly prunes. Disk use rises while the installation has less than 12 months of data and then levels off. DuckDB does not shrink its file on deletion; compaction frees a closed month's blocks after its last slice (0014), which shows as small steps, not a cycle. The forecast below describes that.

## Work

1. Sample sizes daily and at start: each SQLite file, the DuckDB file, the replay store, spill directory size, free disk, and retained analytical records (from `day_aggregates`). Keep 400 days of samples in a small control table; the retention job trims it.
2. Forecast, recomputed once a day:
   - Bytes per retained record from the latest sample.
   - Ingest per day as the median of the last 28 days of eligible events.
   - Days until 12 months are retained, from the oldest retained local day.
   - Steady-state analytics size = bytes per record × ingest per day × 365 + the current SQLite files, plus the replay cap as configured (not the current replay size), plus the disk reserve.
   - The outcome as one of: "levels off near {size} on {date}", "reaches the disk reserve on {date}" when steady state does not fit, or "not enough history" for an installation under 14 days old.
   - Return the inputs with the outcome, so the page can show how it was computed.
3. One query `operator.status` for install admins: collection state, readiness problems, analytics lag, the v02 summary step's counters (runs, skips, timeouts) once v02 lands, disk and recording budgets, last retention, compaction and backup outcomes from `maintenance_jobs`, and the forecast. Everyone else gets `forbidden`.
4. Admin status page: state in words at the top, a disk chart of the samples with the forecast as a dashed continuation and the reserve as a line, the forecast sentence with its inputs, budgets as meters, and the maintenance outcomes as a short table.
5. Sidebar server card for install admins (design contract Server card): state, disk meter, forecast sentence, link to the page. v03 mounts it.

## Acceptance criteria

- With a pinned clock and fixture samples, the forecast's outcome and date match hand-computed values for: a young installation, one at steady state, and one that would hit the reserve.
- A non-admin request is refused and the card and page never render for non-admins, including a workspace owner who is not an install admin.
- The page names every readiness problem in words when the server reports it (test each with the existing health fixtures).
- Sampling does no full scan: it reads file sizes and `day_aggregates` only. Record its run time on the 124M-record fixture.
- No event bodies, identity inputs or site data appear in the status result (install admins read no site data unless they are members).

## Checks

`cargo fmt --all --check`, `cargo clippy --workspace --all-targets`, `cargo test -p holocron-server`, export-types `--check`; from `apps/web`: `bun run typecheck`, `bun run lint`, `bun run test`, an e2e case for admin and non-admin.

## Handoff

Evidence at `docs/evidence/v07-operator-status-and-storage-forecast.md`. v03 mounts the card. Milestone 12 can link the page from its operator workflow.
