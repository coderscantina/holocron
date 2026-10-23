# 0019: Budget presets

Status: Adopted by Mike on 2026-10-21. It adopts item 1 of the shaping list in [0008](0008-storage-engine-and-budgets.md#installation-tiers-sizing-guidance), without the automatic choice.

## Problem

0008 publishes small, standard and large tiers as sizing guidance, but the server always runs the standard budget. An operator on a 1 vCPU host has to know that engine threads, engine memory, Tokio workers and the report lanes belong together, and only the Tokio worker count has a flag.

## Choice

`holocron serve --budget small|standard|large` (env `HOLOCRON_BUDGET`, default `standard`). The preset sets the engine threads and memory limit, Tokio worker threads, report workers and report queue together. The values are the 0008 table:

| Tier | Engine threads | Engine memory | Tokio workers | Report workers | Report queue |
| --- | --- | --- | --- | --- | --- |
| small | 1 | 512 MiB | 1 | 1 | 4 |
| standard | 2 | 1 GiB | 2 | 2 | 8 |
| large | 4 | 2 GiB | 4 | 4 | 8 |

Values the table does not state keep the standard value in every tier: spill (40 GiB at most), inbox backlog (1 GiB), admission queue, batch size, the live lane (1 worker, 8 waiting) and the query timeout. For small, whose disk guidance is 25 GiB, the spill limit is a ceiling and not a reservation; the disk reserve flag stays the protection.

An explicit flag overrides the preset's single value. Today that is `--worker-threads` (`HOLOCRON_WORKER_THREADS`); the disk reserve was already independent of the tier.

The server logs the active tier and its effective values at startup, on the line `resource budget`. The values live in one place, `Budget::sizing` in `apps/server/src/config.rs`; the standard tier reuses the storage default.

## Why the operator chooses

The server does not read the CPU count or the cgroup memory limit. Small and large are estimates: only standard was measured, and small has not run the burst at all. A host that landed on an unmeasured tier because of what it reports about itself would run on numbers nobody has checked, with no sign of it in the configuration. An explicit flag leaves a record in the unit file or environment, and the startup line shows it in the log.

The cost is that an operator who forgets the flag runs the standard budget on whatever host it is. That is the behavior today, so nothing gets worse.

## Not decided here

Items 2 to 4 of the 0008 shaping list stay open: the trailing-12-month warning in `collection.status`, the tier question in `holocron init`, and correction step size from the memory budget. `collection.status` reports no budgets today, so the tier is not added to it.

## Open measurement

Small still needs the 500 events/s burst rerun from 0008: with one engine thread, transfer, correction and reports queue behind each other. Its correction steps need a memory bound before that. Until small and large are measured on a host of their size, they are sizing guidance and not promises. Gate results should record the tier they ran under; the startup line gives it.

## Affected contracts

[0008](0008-storage-engine-and-budgets.md), the [operator runbook](../operations/runbook.md).
