# Implementation milestones

Milestones 01 and 03 are complete; 02 and 04 through 12 are in progress; 13, 14 and 15 are pending. Mark completion only after its acceptance criteria pass and an evidence report is linked here. The [technical briefing](../technical-briefing.md) owns product scope. The [workflow](../development-workflow.md) defines ownership and handoffs.

## Sequence and gates

| ID  | Milestone                                                                      | Dependencies                                      | Suggested owner                | Status  |
| --- | ------------------------------------------------------------------------------ | ------------------------------------------------- | ------------------------------ | ------- |
| 01  | [Contracts and fixtures](01-contracts-and-fixtures.md)                         | None                                              | Integration with domain owners | Complete: [evidence](../evidence/m01-contracts-and-fixtures.md) |
| 02  | [Storage performance gate](02-storage-performance-gate.md)                     | 01                                                | Storage and performance        | In progress: [evidence](../evidence/m02-storage-gate.md) |
| 03  | [Kit shell and generated API](03-kit-shell-and-generated-api.md)               | 01                                                | Frontend and protocol          | Complete: [evidence](../evidence/m03-kit-shell-and-generated-api.md) |
| 04  | [Durable modern collection](04-durable-modern-collection.md)                   | 02, 03 protocol foundation                        | Collection and storage         | In progress: [evidence](../evidence/m04-durable-modern-collection.md) |
| 05  | [Identity, sessions, and corrections](05-identity-sessions-and-corrections.md) | 04                                                | Analytics                      | In progress: [evidence](../evidence/m05-identity-sessions-and-corrections.md) |
| 06  | [Traffic and exploration](06-traffic-and-exploration.md)                       | 03, 05                                            | Frontend and analytics         | In progress: [evidence](../evidence/m06-traffic-and-exploration.md) |
| 07  | [Behavioral reporting](07-behavioral-reporting.md)                             | 06                                                | Analytics and reporting UI     | In progress: [evidence](../evidence/m07-behavioral-reporting.md) |
| 08  | [Retention, revenue, and performance](08-retention-revenue-and-performance.md) | 05, 06                                            | Analytics and reporting UI     | In progress: [evidence](../evidence/m08-retention-revenue-and-performance.md) |
| 09  | [Replay and heatmaps](09-replay-and-heatmaps.md)                               | 03, 04, 05                                        | Recording and frontend         | In progress: [evidence](../evidence/m09-replay-and-heatmaps.md) |
| 10  | [Access and restricted sharing](10-access-and-sharing.md)                      | 03, 06                                            | Identity and frontend          | In progress: [evidence](../evidence/m10-access-and-sharing.md) |
| 11  | [Historical import and cutover](11-historical-import-and-cutover.md)           | 04, 05, 07, 08, 09, 10                            | Import and storage             | In progress: [evidence](../evidence/m11-historical-import-and-cutover.md) |
| 12  | [Operations and recovery](12-operations-and-recovery.md)                       | 04, 05, 09, 10; final import integration after 11 | Storage and operations         | In progress: [evidence](../evidence/m12-operations-and-recovery.md) |
| 13  | [Integrated release gate](13-integrated-release-gate.md)                       | 01 through 12, 14                                 | Integration                    | Pending |
| 14  | [Web setup and onboarding](14-web-setup-and-onboarding.md)                     | 11, 12                                            | Server and frontend            | Pending |
| 15  | [Report queries for agents](15-agent-query-api.md)                             | 06, 07, 08, 10                                    | Analytics, server and frontend | Pending |

02 and 03 run in parallel after the source and wire contracts are defined. The early storage gate authorizes the engine design, not a capacity claim for the completed product. A passing minimal experiment must be repeated with real corrections, authentication, recording, and maintenance in 13.

07, 08, 09, and 10 may run in parallel once their shared types and predecessors are integrated. Implement core authorization in 03 and enforce it throughout. Milestone 10 completes the account, team, 2FA, key, and sharing feature set; it is not permission to postpone authorization.

Importer inspection and fixture preparation may begin after 01. Final mapping and reconciliation depend on the implemented feature schemas. Operations design and failure hooks start with 04; 12 closes their complete operator workflow.

## Open decisions

Decisions waiting for Mike. Remove a line once its record says Adopted.

None.

## Assigning tasks

Split each milestone into interface-sized tasks with explicit file ownership. The acceptance criteria are integrated outcomes, not a list of files that must exist. Read the referenced contract before changing a shared type or policy. The integration owner resolves overlapping migrations and protocol exports.

Milestone closure includes a focused correctness report, relevant UI evidence, updated contract/decision records, and reproducible commands. Large data remains outside Git. Add evidence links beside the completed status.

All current analytics remain in first-release scope. Completing the traffic milestone does not make the first release ready.
