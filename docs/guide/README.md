# Holocron user guide

Holocron is a self-hosted web analytics server for people who run Umami today. One process with one data directory collects events from the unchanged Umami 3.4.0 tracker, stores them in embedded SQLite and DuckDB, and serves the dashboard.

This guide is for the people who install and run an instance, and for the admins who set up sites, teams and tracking inside it.

## Pages

| Page | What it covers |
| --- | --- |
| [Install](install.md) | Docker, docker compose, Coolify, a reverse proxy with Caddy, `holocron init` and the first sign-in |
| [Configuration](configuration.md) | Every command, flag and `HOLOCRON_*` environment variable, proxies and location, the secrets key |
| [Migrate from Umami](umami-migration.md) | `holocron import inspect` and `run`, the tracker cutover, a shadow run with its comparison, what comes over and what doesn't, rollback |
| [Tracking](tracking.md) | The tracker script, data attributes, custom events, identify, revenue, consented profiles, replays and heatmaps |
| [Teams and access](teams-and-access.md) | Workspaces, roles, install admins, two-factor sign-in, API keys, collection keys, share links and AI agents over MCP |
| [Operations](operations.md) | Backup, restore (also in the setup wizard), upgrade and upgrade on start, retention, profile erasure, diagnostics, report queries from a shell, health, logs, the data directory and disk |

The development docs go deeper. The [operator runbook](../operations/runbook.md) and the [import and cutover runbook](../operations/import-and-cutover.md) are the reference for commands and refusal codes. The [decision records](../decisions/) explain why things work the way they do.

## Concepts in one minute

- **Workspaces.** Sites belong to a workspace: a team, which people share, or a personal one. `holocron init` creates a team called Default with the first admin as its owner.
- **Roles.** People get a role in a team: Owner, Admin, Member or Viewer. The install admin manages accounts and server settings and reads a site's analytics only as a member of its workspace.
- **Visitors.** Anonymous visitors get a keyed pseudonym that changes every day per site. Holocron sets no cookie. A site can link visitors who consented to a persistent profile.
- **Collection.** The tracker posts to `/api/send` like it does with Umami. Holocron acknowledges an event only after it is on disk.
- **Retention.** Events are kept for 1 year by default, configurable per installation and per site up to forever. Raw replays are kept 14 days, heatmaps 90 days.

## Getting help

`holocron --help` and `holocron <command> --help` list every option. `holocron diagnostics` prints the state of an instance without event data, identity inputs or credentials, so its output can go into a bug report.
