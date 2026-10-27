# shadow-compare

Checks that Holocron saw the same traffic as Umami during a [side by side run](../../docs/guide/umami-migration.md#run-both-side-by-side-first), before you switch Umami off. It reads both sides and never writes to either.

```sh
cd tools/shadow-compare && bun install
export HOLOCRON_API_KEY=...        # never a flag
export UMAMI_PASSWORD=...          # only if the URL has no password
bun compare.ts --umami mysql://user@db-host/umami --holocron https://analytics.example.com \
  --from 2026-10-23 [--to 2026-11-06] [--site <uuid>]... [--json]
```

`--from` is the first day both sides collected, in each site's timezone. `--to` defaults to yesterday in the site's timezone, so the partial current day stays out. The range is at most 366 days. `--umami` takes a `mysql://`, `postgres://` or `postgresql://` URL. Exit code 0 means everything is within tolerance, 1 means it is not, 2 means bad input or a connection error. `--json` prints the same result for scripts.

## What it checks

Sites are paired by website ID. Holocron keeps Umami's IDs on import, so every site both sides have is compared, or only the `--site` ones. Days are local days in the site's timezone from Holocron (`sites.get`). Umami stores UTC, so each day is read as the exact UTC range of that local day, which also gets 23 and 25 hour days on DST changes right.

| Check | Umami | Holocron (`analytics.explore`) | Tolerance |
| --- | --- | --- | --- |
| Pageviews per day | `website_event` rows with `event_type = 1` | measure `pageviews`, day series | 1% |
| Visitors per day | distinct `session_id` of pageviews and custom events | measure `visitor_days`, day series | 5% |
| Top 10 pages | pageviews by `url_path` | panel `pageviews` by `path` | see below |
| Top 10 countries | pageviews by `session.country` | panel `pageviews` by `country` | see below |

Why these tolerances:

- Pageviews are plain counts of the same hits, so 1% covers requests that reached one side only. A day also passes when the counts are within 2 of each other, so a quiet day is not noise.
- Visitors differ by design. Holocron counts visitor-days (daily pseudonyms, [decision 0003](../../docs/decisions/0003-metric-formulas.md)), Umami counts sessions. The mirror drops Umami's cache token, so Holocron groups visits into sessions by its own rules. The output says so, and the tolerance is 5%.
- Top lists cover the whole range, not each day, and are judged on the set and the order, not the counts. They pass when at least 70% of the entries are in both top 10s and at least 70% of those shared entries sit within 2 ranks of each other. Ties sort by value on both sides.

Days where either side has no pageviews are listed separately as possible outages and are not counted as differences. A day with traffic on one side only still makes the verdict fail, because it is what a gap in the mirror looks like. A day with none on either side does not.

## The API key

Create a key under Account, then API keys, limited to the sites you compare. It needs `analytics.explore`, `sites.get` and `sites.list`. The "Read analytics" preset grants `analytics.report` instead of `analytics.explore` in this tree, so tick the three operations yourself if the preset does not cover them.

The tool uses only `GET /api/queries/analytics.explore`, `sites.get` and `sites.list`, so it works against Holocron releases from 26.10.23 on. It sends one `explore` call per site: the exploration state and two panels as query string arguments, well under the 8 KiB limit.

The Umami login only needs `SELECT` on `website`, `website_event` and `session`.

## Tests

`bun test` covers day bucketing across DST, the tolerances and the top 10 comparison. `bunx tsc --noEmit` type checks.
