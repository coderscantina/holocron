# Milestone 05: identity, sessions, and corrections

## Outcome

Versioned identities and sessions produce deterministic new metrics, including delayed-event correction, without weakening daily anonymity.

## Dependencies and ownership

Requires 04 and settled formulas from 01. Analytics owns derivation and correction; collection owns transient identity inputs and capture context. Read [analytics contract](../analytics-contract.md).

## Work

1. Implement daily site-scoped keyed identities in the configured timezone, daily identify hashing, and separately namespaced consented persistent identities.
2. Keep raw inputs transient. Support the accepted late-event window with the chosen key/date policy. Define consent revocation and avoid implicit cross-day anonymous linkage.
3. Derive configurable inactivity sessions, default 30 minutes, with anonymous midnight closure and deterministic timestamp ties.
4. Separate immutable events/capture IDs from derived sessions. Mark affected ranges dirty, recompute idempotently, and update versioned aggregates.
5. Expose ingestion watermark, correction state, metric version, and session-policy version. Reports cannot claim corrected results while their ranges remain dirty.
6. Implement explicit settings-change/recalculation jobs and identity-population labels needed by retention and historical import.

## Acceptance criteria

- The hand-audited fixture matches all settled formulas and denominators.
- Late events can extend, join, or split sessions correctly, with deterministic replay results.
- Midnight, DST, inactivity boundaries, timezone changes, one-event sessions, and identified mode transitions pass.
- Anonymous identity cannot be used by report operations to construct cross-day person profiles.
- Consent absence or revocation follows the documented population policy.
- Session recalculation leaves recorder chunk order and capture IDs intact.
- Correction work stays bounded and is included in visibility/freshness measurements.

## Evidence and handoff

Publish expected/actual fixture results, privacy-policy cases, policy-version transitions, correction crash cases, and resource traces. Hand stable metric/planner inputs to 06 through 09 and the importer. Metric definition changes update the contract and generated types together.
