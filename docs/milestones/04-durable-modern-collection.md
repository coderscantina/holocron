# Milestone 04: durable modern collection

## Outcome

Unchanged supported modern trackers collect events through Rust and receive stored-event success only after durable local commit. Transfer recovery, refusal, and freshness are observable.

## Dependencies and ownership

Requires 02 and the protocol/startup portion of 03. Collection owns modern wire adapters and canonical validation. Storage owns durable inbox and transfer. Read [the briefing](../technical-briefing.md), collection fixtures from 01, and [performance validation](../performance-validation.md).

## Work

1. Implement modern script/alias serving, send, batch, identify, performance, link/pixel source collection, and recorder-facing context/configuration interfaces. Recording storage closes in 09.
2. Validate exactly one source, typed payloads, byte limits, accepted timestamps, origin policy, and trusted proxy configuration. Preserve supported cache/response shapes.
3. Persist the canonical payload and required collection context in a durable commit. Group commit may combine writes; each success waits for its commit.
4. Implement byte-bounded admission, stable batch assignment, transactional analytical ledger application, retirement, retry, and restart reconciliation.
5. Add optional client-event deduplication using the policy settled in 01. Keep transfer replay and HTTP retry semantics distinct.
6. Expose collection availability, accepted/suppressed/refused outcomes, inbox age/depth, processing lag, and watermark. Keep raw identity inputs and event bodies out of logs.

## Acceptance criteria

- The unchanged pinned modern tracker works through navigation, custom events/properties, identify, performance, batch, links, and pixels.
- Invalid sources, timestamps, payloads, and oversized bodies produce the documented outcomes.
- Every acknowledged event survives the tested durability conditions. A refused event is not counted as stored.
- Transfer interruptions yield neither lost accepted records nor duplicate analytical application.
- Full disk, locked/unavailable storage, and saturated queues produce explicit bounded backpressure, without unbounded memory growth.
- Durable acknowledgement and visibility targets pass the declared collection workload.
- Lost HTTP responses expose retry limits honestly; exactly-once browser delivery is not claimed.

## Evidence and handoff

Run actual embedded engines and supported scripts. Save compatibility results, crash-boundary results, count reconciliation, and load traces. Hand canonical events and correction hooks to 05. Persistent consent extensions remain explicit rather than inferred from unchanged identify calls.
