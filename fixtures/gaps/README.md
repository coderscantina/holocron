# Gap fixtures

Milestone 01. These cover the capabilities the [capability matrix](../../docs/contracts/capability-matrix.md) listed as "no planned fixture", other than access and sharing (see [../access](../access/README.md)). Each file has a `status` naming the decision record that sets its rules, a `grounding` list, and where something is still unnamed, an `open` list. No expectation depends on an `open` item.

## Written

| Capability | File | Rule source |
| --- | --- | --- |
| CAP-TRAFFIC-04 channel classification | `channel-classification.json` | [0003](../../docs/decisions/0003-metric-formulas.md) Channels. 27 sessions, one or more per rule order, lists copied from upstream constants |
| CAP-TRAFFIC-05 weekday and hour grid | `weekday-grid.json` | [0004](../../docs/decisions/0004-exploration-state-and-limits.md) Weekday and hour. Fall-back and spring-forward days, local midnight, session counted per hour |
| CAP-TRAFFIC-08 realtime window | `realtime-window.json` | 0003 Realtime. Activity time, inclusive window, 30 one-minute buckets, keyless events excluded ([0001](../../docs/decisions/0001-identity-and-consent.md)) |
| CAP-TRAFFIC-13, 15 link and pixel totals | `link-pixel-totals.json` | 0003 Links and pixels, 0004 Sources, [0002](../../docs/decisions/0002-sessions-and-events.md) deduplication |
| CAP-EXPLORE-07 segment definitions | `segment-definitions.json` | 0004 Segments and limits. Validity, names, site scope, canonical bytes, apply-by-copy |
| CAP-BEHAV-04 campaign normalization | `campaign-normalization.json` | 0003 Attribution, UTM values |
| CAP-ACCESS-01, CAP-OPS-01 initialization | `init-outcomes.json` | [0006](../../docs/decisions/0006-retention-clock-and-operations.md) `holocron init` outcomes |
| CAP-OPS-07 user erasure | `user-erasure.json` | [0007](../../docs/decisions/0007-access-and-account-security.md) User erasure scope |
| CAP-OPS-09 schema upgrade | `schema-upgrade.json` | 0006 Schema versions |
| CAP-RECORD-04 saved replays | `saved-replays.json` | [0006](../../docs/decisions/0006-retention-clock-and-operations.md) Saved replays (Confirmed). Save under the sub-cap, refusal at it, survival past 14 days, unsave then removal, cap accounting. Chunk format stays with milestone 09 |
| CAP-OPS-10 diagnostics | `diagnostics-canaries.json` | Milestone 12 acceptance criterion. A canary list, not a bundle format |

Limits of these fixtures:

- The segment fixture covers definition validity and canonical bytes. It does not evaluate a segment against events; that belongs to the query suite.
- The init, upgrade and erasure fixtures list cases and expectations in prose. Milestone 12 turns them into runnable procedures. Where a record names no exit code or error code, the case says so.
- The weekday grid fixture asserts non-empty cells only.

## Still deferred

| Capability | Milestone | Why |
| --- | --- | --- |
| CAP-RECORD-03 hostile replay content | 09 | The cases depend on the replay chunk format and playback isolation, which [0006](../../docs/decisions/0006-retention-clock-and-operations.md) leaves to milestone 09. Reserved location: `fixtures/replay-hostile/`. |
| CAP-IMPORT-09 cutover rehearsal | 11 | Needs importer output from 04 and 05. A sequencing dependency, not a missing decision. |
