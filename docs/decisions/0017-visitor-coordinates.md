# 0017: Visitor coordinates for the Realtime globe

Status: Adopted by Mike on 2026-10-18 for the Realtime globe. It extends the geo headers of [0001](0001-identity-and-consent.md) Proxy trust and headers. [0018](0018-geoip-fallback.md) proposes a GeoIP database fallback that replaces the "no GeoIP database" rule below.

## Problem

The Realtime page should show where active visitors are, as pins on a globe. Events store country, region and city as text, which cannot be placed on a globe without a lookup table of cities. Some CDNs already send coordinates next to the city.

## Constraints

- Location comes only from headers a trusted proxy sets. Holocron has no GeoIP database and the client IP is a transient input that is never stored ([0001](0001-identity-and-consent.md) Transient inputs).
- Anything stored about a visitor follows the retention and erasure of the event row ([0006](0006-retention-clock-and-operations.md), [0016](0016-configurable-retention.md)).
- The active count of [0003](0003-metric-formulas.md) Realtime stays the number the page shows. The pins should add up to it.
- Records in the inbox are not self-describing, so a changed record changes the inbox encoding and the analytical schema ([0006](0006-retention-clock-and-operations.md) Schema versions).

## Choice

### Collection

- Latitude and longitude are read from the same provider whose country header matched, and only under the proxy trust rule that already guards country, region and city. A request from an untrusted peer, and a request with a collection key, has no location.

  | Provider   | Headers                                                  |
  | ---------- | -------------------------------------------------------- |
  | Cloudflare | `cf-iplatitude`, `cf-iplongitude`                        |
  | Vercel     | `x-vercel-ip-latitude`, `x-vercel-ip-longitude`          |
  | CloudFront | `cloudfront-viewer-latitude`, `cloudfront-viewer-longitude` |
  | EdgeOne    | none, its geolocation header carries the country only    |

- Both values must be finite numbers, latitude within -90 to 90 and longitude within -180 to 180. If either is missing or invalid, both are dropped. The event keeps its country, region and city.
- Each value is rounded to a tenth of a degree (about 11 km) before it leaves the request. That is no finer than the city lookup the CDN already did.
- Coordinates are never derived from the IP address, and no GeoIP database is added. Amended by [0018](0018-geoip-fallback.md) once adopted: without a trusted country header, they come from the GeoIP database.

### Storage

- `events` gets two nullable `SMALLINT` columns, `latitude` and `longitude`, in tenths of a degree (`482` is 48.2). They live in the same rows as `city`, so retention, erasure, reset and backup treat them the same way. The derived tables do not carry them.
- The inbox format is 3 and the analytics schema is 5. `holocron upgrade` adds the two columns to an existing file without rewriting events or the derived tables. Events stored earlier have no coordinates.
- Inbox rows written by the previous release (format 2) are still read, with no coordinates, so an upgrade does not need an empty inbox.

### CDN settings

The CDN must send the headers, or pins fall back to the country centroid.

- Cloudflare: turn on the "Add visitor location headers" managed transform. It adds `cf-iplatitude` and `cf-iplongitude`, and also `cf-ipcity` and `cf-region-code`.
- Vercel: sends `x-vercel-ip-latitude` and `x-vercel-ip-longitude` by default.
- CloudFront: use an origin request policy that forwards `CloudFront-Viewer-Latitude` and `CloudFront-Viewer-Longitude`, and the city and region headers.
- EdgeOne: no coordinate header exists, so every pin is a country centroid.

### Realtime

- `RealtimeSnapshot.active_locations` lists the visitors of the five-minute active window ([0003](0003-metric-formulas.md) Realtime), each at the place of their latest event in the window. Visitors are grouped by country and 0.1 degree cell. A location carries the visitor count, the city and the path of its latest event.
- A visitor whose latest event has coordinates and a country is placed there. A visitor with a country but no coordinates is placed at the centroid of the country, without a city. A visitor without a country, or in a country without a centroid, has no pin.
- The fallback runs on the server, from a static table of country centroids in `holocron-analytics`. A client-side fallback would need the same table in the browser and could not keep the pins equal to the server's count. With it, the pins add up to the active count, minus visitors without a country and keyless events.
- `RecentEvent` gains `city`, `lat` and `lng` by the same rule, so the event list and the globe agree. Without a country they are absent, and `city` is absent at a centroid.

## Rejected alternatives

- **A GeoIP database.** It needs a licence and updates, and it puts the client IP into a lookup on every event. The CDN has already done that lookup.
- **Browser geolocation.** It asks the visitor for permission, which the tracker never does today.
- **Storing the headers' full precision.** A globe pin gains nothing from it, and the extra digits would be stored for the whole retention.
- **A table of coordinates per city.** It needs data to be kept current and fails for cities the CDN spells differently. The header already has the point.
- **Centroid fallback in the browser.** See Realtime above.
- **A separate coordinates table.** Coordinates belong to the event like the city does. A join would only be needed to erase them.

## Consequences

- Without the CDN settings above, every pin sits at a country centroid. The globe works but shows no cities.
- A country centroid pin can sit in the middle of a country where nobody lives. That is the honest reading of "somewhere in this country".
- A backup that still holds waiting format 2 inbox records is refused by this release's restore, as for any other format difference ([0014](0014-operations-and-recovery.md)). Let the inbox drain before the backup, or restore with the release that took it.
- A batch that was written to the ledger but not yet removed from the inbox when the old release stopped keeps its old schema and format numbers, and replay refuses it ([0006](0006-retention-clock-and-operations.md) Schema versions). Only a crash between the ledger write and the inbox delete, followed by the upgrade, leaves one.
- Imports from Umami carry no coordinates. This only matters for events in the last 30 minutes, which an import does not contain.

## Affected contracts

0001 Proxy trust and headers, 0003 Realtime, `events` in the analytical schema (`ANALYTICS_SCHEMA` 5), `INBOX_FORMAT` 3, protocol `RealtimeSnapshot.active_locations`, `ActiveLocation` and `RecentEvent`.
