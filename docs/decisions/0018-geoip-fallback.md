# 0018: GeoIP database as a location fallback

Status: Adopted by Mike on 2026-10-21. It amends [0001](0001-identity-and-consent.md) Proxy trust and headers and [0017](0017-visitor-coordinates.md) Collection, which ruled out a GeoIP database and coordinates from one.

## Problem

Location comes only from CDN headers sent by a trusted proxy. Behind a plain reverse proxy, reports show no country, region or city and the Realtime globe has no pins. Umami ships a GeoLite2 City database (`GEOLITE_DB_PATH`, `scripts/build-geo.js`), so people moving from Umami lose location they had.

## Constraints

- The client IP is a transient input, never stored or logged ([0001](0001-identity-and-consent.md) Transient inputs).
- An untrusted peer must not choose its address or location. A request with a collection key has no location ([0001](0001-identity-and-consent.md) Proxy trust and headers, collection keys).
- Coordinates are rounded to a tenth of a degree before they leave the request ([0017](0017-visitor-coordinates.md)).
- An optional lookup must never refuse or fail an event ([0005](0005-collection-admission-policy.md)).
- Memory headroom on the reference host is thin: up to 3.84 GB of 4 GiB at peak ([0008](0008-storage-engine-and-budgets.md)).
- GeoLite2 needs a MaxMind account, a licence key and its EULA, which forbids redistribution. DB-IP's IP to City Lite is free under CC BY 4.0, without an account.

## Evidence

Checked on 2026-10-19:

- `https://download.db-ip.com/free/dbip-city-lite-YYYY-MM.mmdb.gz` serves a gzip MaxMind DB per month. The October 2026 file is 60 MB packed and 127 MB unpacked, with `Last-Modified` 1 October 06:29 UTC. The November URL answers 404.
- The file's `database_type` is `DBIP-City-Lite`. Records carry country ISO code, English city name, latitude and longitude. Subdivisions have names but no ISO code, and there is no accuracy radius.
- The licence on DB-IP's download page: attribution to DB-IP.com, and "in the case of a web application, you must include a link back to DB-IP.com on pages that display or use results from the database". The suggested markup is `<a href='https://db-ip.com'>IP Geolocation by DB-IP</a>`.
- Upstream `src/lib/detect.ts`: CDN headers first, then the database. Country is `country.iso_code`, else `registered_country.iso_code`. Region is the first subdivision's ISO code, city the English name.
- `maxminddb` 0.32 reads the file. `Reader::verify` on the October file took 0.7 s and about 30 MB on top of the file on a laptop.

## Choice

### Lookup

- A trusted CDN country header still decides, with its region, city and coordinates, as before.
- Without one, the database is asked for the client address that 0001 already picks: the peer address when the peer is not a trusted proxy, otherwise the forwarded address. A request with a collection key gets no location, as before.
- Country is `country.iso_code`, else `registered_country.iso_code`, as upstream. Region is `CC-RR` from the first subdivision's ISO code, so DB-IP Lite gives none. City is the English name, cut at 50 characters. Latitude and longitude are rounded and checked exactly as 0017 requires, and dropped as a pair when invalid.
- No record, a private address, or a lookup error means no location. The address stays in request memory and the lookup logs nothing.
- Every path that resolves a visitor uses it: send, batch, link and pixel hits, and the country of a replay part.

### Where the database comes from

- `HOLOCRON_GEOIP_DB` (`serve --geoip-db`) names a MaxMind DB City file: GeoLite2 City, GeoIP2 City or DB-IP Lite City. It is read into memory and verified at start. It is not reloaded, so a new file needs a restart. A missing, unreadable or non-City file logs a warning and the server runs without lookups. While the setting is present it wins, and the dashboard shows the file as managed by the server configuration.
- Otherwise an install admin downloads DB-IP Lite City from the administration area with `geoip.download`. The server tries the current UTC month, then the previous one. It streams and unpacks into a temporary file in `<data>/geoip/`, stops past 512 MiB unpacked, opens the result, checks that its type names a City database and verifies it. Only then is it renamed to `dbip-city-lite-YYYY-MM.mmdb`, swapped in for new lookups and the older month removed. No restart is needed.
- The download starts only with the disk reserve plus 512 MiB free. A failure (unreachable, not published, invalid file, disk full) is shown with its reason, and the database already installed keeps serving.
- The downloaded file is memory-mapped, so it counts as file cache, not heap. That is safe because Holocron never writes an installed file in place: a new month is a new file. The operator's file is read into memory instead, because an in-place copy over a mapped file would crash the server.
- Updates are manual. `geoip.settings` shows the installed month and the newest published month. It looks the latter up with a HEAD request only when the installed month is older than the current one, and keeps the answer for an hour. No background job contacts DB-IP.

### Attribution

- While the loaded database is DB-IP's (`database_type` starts with `DBIP`), `/config.json` has `dbip_attribution: true`. The dashboard and shared pages then show "IP geolocation by DB-IP" with a link to `https://db-ip.com`, which covers every page that shows location. The administration section and the docs name it too.

### Backups

- `<data>/geoip/` is not part of a backup, since the file can be downloaded again. After a restore, download it again or keep `HOLOCRON_GEOIP_DB`.

## Rejected alternatives

- **Bundling a database in the image.** GeoLite2's EULA forbids it. DB-IP would allow it, but it adds 60 MB to the image and goes stale between releases.
- **A GeoLite2 download with a MaxMind licence key.** Every operator would need an account and a key. Operators who have one can run `geoipupdate` and point `HOLOCRON_GEOIP_DB` at its file.
- **A monthly update job.** It contacts a third party without anyone asking. Mike chose a manual update.
- **Lookups for requests with a collection key from `payload.ip`.** 0001 gives them no location, and this decision does not reopen that.
- **Loading the downloaded file into the heap.** 127 MB of anonymous memory, twice that during a swap, on a host with thin headroom.
- **Reloading the operator's file when it changes.** A restart is simple and rare. Watching the file adds a moving part for a monthly event.

## Consequences

- Without a CDN, reports get country, city and globe pins. DB-IP Lite has no region codes, so region reports stay empty under it. Its accuracy is below the commercial database's, by DB-IP's own index 77 against 96.
- A visitor whose connection is not from a trusted proxy now gets the location of the peer address. Behind a proxy that is not listed in `HOLOCRON_COLLECT_TRUSTED_PROXIES`, every visitor gets the proxy's location. A proxy on a private address, as in Docker, has no record, so they get none.
- Events stored before a database was installed keep no location. Nothing can be backfilled because addresses are not stored.
- Pressing download, and opening the setting while an older month is installed, makes an outbound request to `download.db-ip.com`.
- `/config.json` is read when the dashboard loads, so the attribution appears or disappears at the next page load after a change.
- Imports from Umami are unchanged: they copy Umami's stored location.

## Affected contracts

0001 Proxy trust and headers, 0017 Collection, the `serve` setting `HOLOCRON_GEOIP_DB`, the data directory entry `geoip/`, protocol `geoip.settings` and `geoip.download` with `GeoipSettings`, `GeoipDatabase`, `GeoipDownload` and `GeoipFailure`, `RuntimeConfig.dbip_attribution`, and the access matrix (`fixtures/access/cases.json`, AX-G1326 to AX-G1349).
