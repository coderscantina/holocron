# 0011: Replay store, capture identity, and playback isolation

Status: Adopted (milestone 09, team decision) unless an item says otherwise. Items marked Confirmed were decided by Mike. Evidence: [m09 replay and heatmaps](../evidence/m09-replay-and-heatmaps.md).

## Problem

Milestone 09 needs a place for replay parts that a 25 GiB cap can be enforced against at admission, a unit replays are kept and removed by, a heatmap aggregate, and a way to render captured pages without giving them the dashboard's privileges. [0006](0006-retention-clock-and-operations.md) left the replay format and playback isolation to this milestone; [0008](0008-storage-engine-and-budgets.md) asked for compressed chunks.

## Constraints

Confirmed: recording off by default per site, strict masking, sensitive-page and element exclusions, 10 % replay sampling; 14-day raw replay, 90-day heatmaps, 25 GiB cap; budgets enforced during admission; capture IDs kept apart from derived sessions; ordinary collection stays available at the cap; saved replays exempt from 14 days within a 5 GiB part of the cap (0006). The unchanged Umami 3.4.0 recorder must keep working.

## Evidence

- The recorder sends one request per part, with a strictly increasing part index in Unix seconds, and never retries (`src/recorder/index.js:78-128`). It splits events over 500,000 bytes into `umami:rrweb-event-fragment` events sent as consecutive parts.
- 0008 measured replay as 16.4 GB of uncompressed DuckDB BLOBs, and a gzip ratio of 11.75 on the template stream. The m09 measurement compresses rrweb-shaped parts 17 times with zstd level 3.
- rrweb's Replayer renders into its own iframe with `sandbox="allow-same-origin"` and reads that document. In a frame on an opaque origin (`sandbox="allow-scripts"` without `allow-same-origin`), the inner iframe inherits the sandbox and gets a second opaque origin, so `contentDocument` is null and playback fails ("createSandboxedIframe() requires root to be connected"). Checked in Chromium 1243.
- With the frame's content security policy, Chromium blocks every image, stylesheet, frame and fetch of the hostile fixtures before any request leaves the browser (Playwright reports the failure as `csp`; a route on the beacon host is never reached).

## Choice

### Replay store

- Replay parts live in their own SQLite file, `recordings.sqlite`, not in the event inbox and not in DuckDB. One write transaction checks the capture's exclusion, the duplicate key and the cap, stores the zstd-compressed part, and updates the capture summary and the stored byte count. The answer to the recorder comes after that commit.
- The cap counts compressed part bytes. A part that would cross it is refused with `{ok:false, reason:"replay_budget_exceeded"}` and the capture is marked truncated. Nothing is evicted to make room; retention frees space. Heatmaps and events do not count against it.
- The cap is configurable (`--recording-cap-mib`, default 25,600). Saved replays may hold a fifth of it, which is 5 of the default 25 GiB.
- Rejected: replay through the event inbox into DuckDB. The cap check would sit in front of a queue rather than inside the commit, multi-megabyte BLOBs would share the 1 GiB inbox budget and the single analytical writer with events, and freed DuckDB blocks do not shrink the file. SQLite with incremental vacuum gives removed replays back to the file system.

### Capture identity

- A capture is the visit of the recorder's session token: `(site, visitId)`. It stores the token's daily pseudonym as `visitor`, never an analytical session ID. Lists and playback find the analytical session that contains the capture's first part at read time, so corrections never move a replay; they move only the link.
- Part order is the recorder's part index, then arrival. The same bytes at the same index are one part (`duplicate`, stored once). Different bytes at the same index (two tabs of one visit) are both kept. Split events are restored at playback, in any arrival order; one that misses a fragment is dropped and counted as a gap. A part arriving after removal starts a new capture without a page snapshot, which plays as "cannot be played".
- Imported recordings (milestone 11) map onto the same keys: site, upstream visit ID, chunk index.

### Exclusions

- Element exclusions are the recorder's `blockSelector`: the browser leaves those elements out before anything is sent.
- The unchanged recorder cannot skip a page. Page exclusions are path patterns the server applies: a heatmap event on an excluded path is dropped; a replay part that reaches one deletes everything stored for that capture and every later part of it is dropped (`page_excluded`). The visitor's browser has already sent that data; product documentation must say so.

### Heatmaps

- Interactions travel through the inbox into DuckDB like events. Once a capture has received nothing for an hour, its interactions fold into per-day cells and are deleted: clicks by viewport class (under 768, under 1280, wider), one of 64 columns of the viewport width and 32-pixel rows of the page; scroll depth as the deepest decile each page visit reached. Reads add the interactions not folded yet.
- Confirmed (Mike, 2026-10-04): cell days are UTC dates and removal is at UTC day `today − 90`, instead of the site-local date 0006 names. The difference is at most 14 hours at either end of the window.

### Playback isolation

- Captured pages render only in `/replay-frame.html`, a separate page served with `default-src 'none'`, `img-src data: blob:`, `connect-src 'none'`, `frame-src 'none'`, `form-action 'none'` and the sandbox flags `allow-scripts allow-same-origin`. The browser applies the same policy to rrweb's iframe, so captured content cannot load anything from the network.
- rrweb's iframe allows no scripts and the frame gives it no pointer input. The server strips scripts, event handler attributes, script URLs, framed documents, link targets and SVG animations that set handlers from every playback page as well ([`fixtures/replay-hostile/`](../../fixtures/replay-hostile/)).
- The dashboard fetches pages through the API and passes event JSON to the frame with `postMessage`; both sides check the sender and the message shape.
- Confirmed (Mike, 2026-10-04): the frame keeps the dashboard's origin, because rrweb cannot render inside an opaque-origin sandbox (Evidence). Captured content never runs script, so it cannot use that origin, but the separation rests on three layers instead of the origin boundary. Holocron serves one host name: there is no setting for a separate replay host. Full origin isolation would need one, and Mike rejected the second host name as setup every operator would carry for a layer the other three already cover.

## Consequences

- Backups (milestone 12) must snapshot `recordings.sqlite` with the control database, the inbox and DuckDB. Site deletion must remove its captures; maintenance already removes every capture of a site that no longer exists.
- Operations status (milestone 12) can read the budget from `recordings.budget`.
- Restricted shares with a recordings grant (milestone 10) must filter `recordings.list` and `recordings.get` by the share's range and pinned filter.

## Affected contracts

[Collection compatibility](../contracts/collection-compatibility.md) `POST /api/record`; [capability matrix](../contracts/capability-matrix.md) CAP-COLLECT-12 to 14 and CAP-RECORD-01 to 06; [0006](0006-retention-clock-and-operations.md) Replay storage format; [0008](0008-storage-engine-and-budgets.md) replay layout.
