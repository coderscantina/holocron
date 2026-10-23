# Changelog

All notable changes to Holocron are documented here.
Commits follow the [Gitmoji](https://gitmoji.dev/) convention.

## 26.10.23

The first public release, in beta.

- **Set up in the browser.** Start the image on an empty volume, enter the setup token from the log, and the wizard creates your admin and first site, or imports from Umami with a preview, live progress and a reconciliation report. No shell needed.
- **Move from Umami.** Imports Umami 3.4.0 on PostgreSQL and 2.20.2 on MySQL, keeps website IDs, logins and authenticator apps, and serves the unchanged Umami tracker. A Lambda@Edge mirror lets both run side by side behind CloudFront before the switch.
- **Analysis Umami doesn't have.** Today against a typical weekday with a projection, noticed changes, nested filters, funnels, journeys, retention, revenue, replays and heatmaps.
- **One process, one data directory.** Embedded SQLite and DuckDB, verified backup, restore and upgrade commands, and a Docker image for amd64 and arm64.

### Features

- 🎉 holocron, self-hosted web analytics for umami operators

