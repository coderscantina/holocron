# Changelog

All notable changes to Holocron are documented here.
Commits follow the [Gitmoji](https://gitmoji.dev/) convention.

## 26.10.28

### Features

- ✨ project net revenue per currency under the typical weekday
- ✨ replace the revenue globe with a summary of pace, refunds and paying traffic
- ✨ show the top events as a board of daily strips
- ✨ split funnels by a field, time their steps and match globs
- ✨ accept queries as post with a json body
- ✨ answer report queries for agents in named blocks
- ✨ send report queries from the command line
- ✨ serve report queries to agents over mcp
- ✨ offer a read analytics key preset and glob filters
- ✨ split a funnel by a field and show the time between steps
- ✨ compare a shadow run of umami and holocron per site and day
- ✨ add the entry page as a session field and a revenue source
- ✨ restore a backup in setup mode, with the upgrade as a second phase
- ✨ add the restore steps to the setup wizard
- ✨ upgrade on start when the compose file asks for it

### Fixes

- 🐛 count a referenced fan-out by its real values
- 🐛 keep the step duration popover above the ribbon
- 🐛 finish the restore step when setup mode closes under it
- 🐛 let sign-in and setup follow the system color scheme

### Security

- 🔒 list only member sites to an api key

### Interface

- 🌐 write german durations as tg., std., min., sek.
- 💄 keep the guest bars clear of a wide setup column
- 💬 tell a refused sign-in where to open holocron

### Other changes

- 📝 record milestone 15 and the revenue and events branch in the status
- 📝 brief report queries for agents as milestone 15 and decision 0022
- ✅ let a read-only key list the sites it reaches
- 📝 adopt report queries for agents and settle the funnel split and duration design
- ✅ cover report cuts over a hundred rows
- 🔧 give refused cli queries their own exit code
- 📝 record milestone 15 as complete and waiting to merge
- ✅ run the browser journeys with one script
- 🚨 drop an unneeded mut in the entry page test
- 📝 record the evidence for milestone 15 and mark it complete
- 📝 record the cleanup wave and the production state in the status
- 📝 name the setup token as the bootstrap credential
- ✅ restore a seeded backup through the setup wizard in e2e
- 📝 describe restoring a backup in the setup wizard
- ✅ capture setup screens after the scheme switch settles
- 📝 record milestone 14 evidence and the restore wave in the status
- 📝 document upgrade on start and its rollback
- 📝 record upgrade on start in the status
- 📝 list agents, restore in the wizard and upgrade on start in the readme

## 26.10.26.2

Dependency updates for the web app, the server and CI, with DuckDB kept on 1.5. Release 26.10.26 was tagged but never published: its CI failed on DuckDB 2.0, which the update had picked up. This release replaces it and carries its updates.

### Other changes

- 📌 keep duckdb on 1.5 until the engine upgrade is its own change

## 26.10.26

Dependency updates for the web app, the server and CI. No change in behavior.

### Other changes

- ⬆️ update web dependencies, vueuse 15 and vitest 5
- ⬆️ update rust dependencies
- ⬆️ update github actions and ci's bun to 1.4.4

## 26.10.23.2

Fixes an Umami import that stopped at its reconciliation step with "read cancelled after the query timeout" on larger sources or slower hosts. Resume the stopped import after updating, or discard it and start again if the source kept collecting.

### Fixes

- 🐛 keep the dashboard's 30 s read limit from stopping an import at reconciliation

### Other changes

- 📝 record the first release in the status
- 👷 number a second release on the same day yy.m.d.2

## 26.10.23

The first public release, in beta.

- **Set up in the browser.** Start the image on an empty volume, enter the setup token from the log, and the wizard creates your admin and first site, or imports from Umami with a preview, live progress and a reconciliation report. No shell needed.
- **Move from Umami.** Imports Umami 3.4.0 on PostgreSQL and 2.20.2 on MySQL, keeps website IDs, logins and authenticator apps, and serves the unchanged Umami tracker. A Lambda@Edge mirror lets both run side by side behind CloudFront before the switch.
- **Analysis Umami doesn't have.** Today against a typical weekday with a projection, noticed changes, nested filters, funnels, journeys, retention, revenue, replays and heatmaps.
- **One process, one data directory.** Embedded SQLite and DuckDB, verified backup, restore and upgrade commands, and a Docker image for amd64 and arm64.

### Features

- 🎉 holocron, self-hosted web analytics for umami operators

