# Milestone 09: replay and heatmaps

## Outcome

Supported modern recorder scripts produce usable, masked replay and heatmaps under bounded storage without compromising ordinary analytical collection.

## Dependencies and ownership

Requires 03, 04, and 05. Recording owns wire validation, fragment storage, playback, aggregation, and budgets. Frontend owns the Kit replay explorer and heatmap screens. Read the recorder fixtures from 01 and privacy/retention requirements in [the briefing](../technical-briefing.md).

## Work

1. Implement recorder configuration, session-cache handshake, fragments, ordering, duplicate handling, replay listing/playback, and modern heatmap payloads.
2. Default recording off per website. Enforce strict text/input masking in supported script configuration, sensitive-page/element exclusions, 10% sampling, and explicit policy state.
3. Keep stable capture IDs distinct from derived analytical sessions. Define behavior for fragment gaps, truncation, late chunks, and imported recordings.
4. Enforce 14-day raw replay, 90-day aggregated heatmaps, and the 25-GiB recording cap. Bound ingress buffers, decoded payloads, aggregation work, and playback responses.
5. Provide replay filtering, session/report links, playback controls, accessible event timeline, and heatmap view controls using Kit components.
6. Isolate recorder rendering from dashboard privileges. Treat replay documents as untrusted content and prevent active captured scripts or network behavior from executing in the application context.
7. Add recording workloads to ingestion, maintenance, permissions, backup/restore, and final capacity tests.

## Acceptance criteria

- The unchanged supported modern recorder passes end-to-end capture and playback fixtures, with deliberate deviations documented.
- Default disabled policy transmits/retains no accepted recording. Supported strict masking and exclusions are verified in the browser before transmission.
- Cap exhaustion visibly refuses/suppresses recording according to its documented protocol outcome while ordinary events remain collectible.
- Fragment recovery, reordering, and duplication do not corrupt playback or create duplicate capture data.
- Analytical session corrections preserve replay sequence.
- Retention/cap cleanup and heatmap aggregation remain bounded at declared load.
- Replay and heatmap reads enforce SiteScope and restricted sharing permissions.

## Evidence and handoff

Publish privacy capture cases, hostile-content cases, recorder compatibility, fragment recovery, cap/retention measurements, and Kit browser evidence. Server settings do not prove arbitrary clients masked their payloads; operator documentation states that limit. Hand mapping details to 11 and maintenance rules to 12.
