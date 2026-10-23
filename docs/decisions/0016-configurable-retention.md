# 0016: Configurable event retention and tolerated MySQL schema drift

Status: Adopted by Mike on 2026-10-17. It amends the 12-month event retention of [0006](0006-retention-clock-and-operations.md) and the import cutoff and exact schema match of [0013](0013-historical-import.md).

## Problem

Umami keeps every event forever. An operator moving three years of history to Holocron lost everything older than 12 months, and no site could keep more. A real Umami 2.20.2 MySQL database was also refused, although every value read the same: its columns had been recreated as `datetime(3)`, signed `int` and other `varchar` lengths, which Prisma produces with `db push`.

## Choice

### Retention values and scope

- A retention is one of `1y`, `2y`, `3y`, `5y`, `10y` or `forever` (protocol `Retention`). The cutoff rule of 0006 stays: the start of the local date `today` minus that many calendar months, clamped to the month's last day. `forever` has no cutoff.
- The installation has a default, `1y` when the installation is created. Install admins change it with `retention.updateDefault`. It is stored in `installation_settings.event_retention`.
- A site may set its own value with `sites.updateRetention`, which needs `site.delete`, so workspace owners and admins can use it. Without one the site follows the default. Links and pixels follow the default, except where an import wrote their value. There is no UI for setting it per link or pixel.
- Only analytical events follow the setting: events, properties, sessions, performance, revenue and identify data. Raw replay keeps its 14 days and heatmaps keep their 90 days, under their own caps.

### Changing it

- Lengthening is a plain setting change. Data already removed does not come back.
- Shortening needs a password check within the last 10 minutes ([0007](0007-access-and-account-security.md)). `sites.retentionPreview` and `retention.previewDefault` first show how many stored events the change removes and the new cutoff. Queries apply the new cutoff at once, so results never depend on the job, as 0006 requires. The hourly retention job then removes the rows.
- Changing the default applies to every source without its own value. The default preview sums over those sources.

### Queries

- A relative range may reach back as far as the site's retention. With `forever` it has no limit beyond the Unix epoch. The fixed 366-day limit on relative ranges is gone.
- A new range `all` covers every retained day through today. It starts at the later of the retention cutoff and the site's first stored event.
- Shares keep their 1 to 366 days and can never use `all`. Session-policy recalculation keeps its 366 days.
- Reads with `forever` use the Unix epoch as their lower bound.
- The cohort retention report omits `history_start` when the site keeps every event.

### Import

- `holocron import run --retention <value>` writes the value onto every imported site, link and pixel. Events before its cutoff are not imported. The default is `forever`, so an import keeps the whole history and the hourly job does not remove it later. A resume needs the same value. Runs started before the option existed resume with `1y`, which is what they used.
- `holocron import inspect` reports `preview.events.outside_retention` per value, together with each cutoff.

### Tolerated MySQL schema drift

- On MySQL, a column whose type differs from the 2.20.2 snapshot is accepted when both types belong to one family:
  - `timestamp` and `datetime` at any precision
  - one integer type of either sign
  - `char` and `varchar` of any length

  Nullability must still match, and every other difference is still refused with `column_changed`. PostgreSQL 3.4.0 keeps the exact match.
- `inspect` lists every tolerated column under `tolerated` and says that `datetime` values are read as UTC. They carry no timezone, and Umami writes them in UTC. The importer already reads every value through SQL casts in a UTC session, so no reader changed.
- Imported text is cut to the length the snapshot declares for its column. A widened source column yields no value longer than Umami 2.20.2 writes, and collection applies the same limits.

## Rejected alternatives

- **A workspace-level default between the installation and the site.** It adds a third layer to explain and to resolve on every request, for no case anyone asked for.
- **Any number of months.** It is harder to size and test. Shortening below 12 months for privacy reasons can be added as a value when it is needed.
- **Downsampling old years to aggregates.** Property and session drill-downs would stop working on old data, and the briefing's exact counts would need a second meaning.
- **Importing everything regardless of retention.** With a finite site setting, the next hourly run would remove the old rows again.
- **Accepting any type the casts can read.** This would also accept a `text` column where Umami bounds a length, and a schema nobody has checked.

## Consequences

- Disk and query cost grow with retained history, not with one year. The sizing tiers in [0008](0008-storage-engine-and-budgets.md) and the p95 targets were measured on 12-month ranges. Multi-year latency is not measured yet; that needs a multi-year workload on the reference host. The `collection.status` tier warning 0008 sketches is still not adopted, so nothing warns an operator who keeps several years on a small tier.
- `imported_history` is not trimmed by retention, so after a shortening "Includes Umami history through" can name an older date than the data that remains. This predates this decision.

## Affected contracts

0006 Retention cutoff, 0013 Inspection and History and identity, the [import and cutover runbook](../operations/import-and-cutover.md), the access matrix (`fixtures/access/cases.json`, AX-G1160 to AX-G1181 and AX-G1350 to AX-G1387), protocol `Retention`, `Range::All`, `Site.retention`, and the `sites.*` and `retention.*` operations.
