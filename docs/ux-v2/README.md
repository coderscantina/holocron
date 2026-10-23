# Dashboard v2

v2 reworks the dashboard so the overview tells an operator how today compares with a normal day, where it is heading, and which few changes explain the difference. It adds a typical-weekday comparison, labelled pace projections, deterministic findings computed in the background, per-site status in the top bar, grouped navigation, an operator status page, and a new shell: one rounded canvas inset into a frame, with filled controls and hairline panels.

v2 is an overhaul of existing screens, not a new product. Every milestone 06 to 10 behavior stays: exploration state, URL roundtrip, saved views, boards, sharing, permissions. All analytics in [the technical briefing](../technical-briefing.md) remain in scope.

## Read in this order

1. [Design contract](design.md): screens, states, copy, and what v2 does not take from the inspiration screens.
2. [0015](../decisions/0015-baselines-projections-and-noticed-changes.md): baseline, projection and findings rules. Adopted 2026-10-15.
3. The brief you are assigned, below. Each one lists the files it owns and the checks that close it.
4. [Development workflow](../development-workflow.md) for ownership, shared changes and the completion report. It applies unchanged.

The visual reference is [examples/v2/index.html](../../examples/v2/index.html): open it directly, no build. It uses sample values and does not calculate anything. [examples/v2/rejected/first-draft.html](../../examples/v2/rejected/first-draft.html) is the rejected first draft; do not build from it.

## Briefs

| ID | Brief | Depends on | Owner | Status |
| --- | --- | --- | --- | --- |
| v01 | [Typical-weekday baseline and pace projection](v01-baseline-and-projection.md) | none | Analytics and protocol | Built and merged; with `typical_weekday` 11 of 31 full-scale 30-day cases miss 2 s p95, and the hour-total rule awaits Mike |
| v02 | [Site summaries and findings](v02-site-summaries-and-findings.md) | v01 | Analytics and server | Built and merged; workload measured in v09 (3 to 4 % engine time at its cadence, burst passes) |
| v03 | [Shell, top bar, site menu and grouped navigation](v03-shell-and-navigation.md) | none for the shell and grouping; v02 for pace and badges | Frontend | Built and merged; light-mode pace chip contrast (4.04:1) awaits Mike |
| v04 | [Overview v2](v04-overview.md) | v01, v03; v02 for findings | Frontend | Built and merged; copy questions open |
| v05 | [Finding actions and replay filters](v05-finding-actions-and-replay-filters.md) | v02, v04 | Recording and frontend | Built and merged |
| v06 | [Workspace sites page](v06-workspace-sites-page.md) | v02, v03 | Frontend | Built and merged; grouped and stale rows covered by unit tests only |
| v07 | [Operator status and storage forecast](v07-operator-status-and-storage-forecast.md) | none | Storage, operations and frontend | Built and merged; sampling run time on the 124M fixture not recorded |
| v08 | [Command palette results](v08-command-palette.md) | none; v02 for findings | Frontend | Built and merged |
| v09 | [v2 acceptance](v09-acceptance.md) | v01 to v08 | Integration | In progress: journeys, visual and accessibility pass; full-scale latency gate fails (diagnosed, needs a decision) |

## Gates

[0015](../decisions/0015-baselines-projections-and-noticed-changes.md) was adopted on 2026-10-15, so no brief waits for a decision. Briefs wait only for the briefs in their Depends on column.

The `previous_period` "until now" fix listed in v01 is a correction of current behavior rather than a v2 feature, adopted in [0003](../decisions/0003-metric-formulas.md) Comparison and zero baselines. Ship it as its own change.

## Parallel work and file ownership

Assign one owner per shared file. The protocol owner merges every change in `crates/protocol` and regenerates `apps/web/src/generated`; the storage owner orders migrations.

| Area | Owner brief | Others may |
| --- | --- | --- |
| `crates/protocol/src/exploration.rs`, `operations/analytics*` | v01 | read |
| `crates/protocol/src/operations/` new `summaries` module | v02 | read |
| `crates/protocol/src/operations/recordings*` | v05 | read |
| `crates/protocol/src/operations/` operator status | v07 | read |
| `crates/analytics/src/explore/` (resolve, compare, projection) | v01 | read |
| `apps/server/src/background.rs`, new `summaries/` module, control migration for summaries | v02 | read |
| `apps/server/src/recordings/` list filter | v05 | read |
| `apps/server/src/maintenance/` status and storage samples, control migration for samples | v07 | read |
| `apps/web/src/layouts/AppLayout.vue`, `components/app/*`, `components/PageHeader.vue`, `composables/useNavigation.ts`, `lib/navigation.ts`, surface and geometry tokens in `assets/css/app.css`, `ui/button` and `ui/card` variants | v03 | read |
| `apps/web/src/pages/overview/*`, `components/exploration/TrendChart.vue`, `MeasureCards.vue`, `ChangeBadge.vue`, `ExplorationControls.vue`, `ExplorationHeader.vue`, `RangeMenu.vue`, default state in `services/exploration.ts`, series tokens in `assets/css/app.css` | v04 | read |
| `apps/web/src/components/findings/*` (new) | v05 | v04 mounts it |
| `apps/web/src/pages/workspace/*` sites page | v06 | read |
| `apps/web/src/pages/admin/*` status page | v07 | read |
| `apps/web/src/components/CommandPalette.vue` | v08 | read |
| `apps/web/src/i18n/en.json`, `de.json` | each brief adds its own keys under its own prefix; the frontend owner resolves conflicts | |

The working tree carries uncommitted changes to the shell, exploration controls and overview (`git status` on 2026-10-15). Agents start from the committed `main` plus those changes once Mike commits them. They do not reset or rewrite them.

## Agent briefing rules

Each brief is written for an agent that knows nothing of the conversation that produced it.

- Read the brief, the design contract section it names, 0015 where it applies, and the current code it lists before changing anything. The code survey in each brief describes the tree on 2026-10-15; check it still holds.
- Stay inside the brief's file ownership. A needed change to another brief's file goes to its owner as a request, with the reason.
- Make targeted edits. Do not rewrite a working component to restyle it.
- Generated files are regenerated with `cargo run -p holocron-protocol --bin export-types`, never edited.
- A threshold, label or rule that 0015 does not settle is a question for the integration owner, not a local choice.
- Report with the completion report in the [development workflow](../development-workflow.md#completion-report), including checks that could not run and why.
