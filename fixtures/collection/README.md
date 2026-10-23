# Collection fixtures

One JSON file per case for every public collection surface of Umami 3.4.0, plus browser captures of the real tracker and recorder. The contract they test is [docs/contracts/collection-compatibility.md](../../docs/contracts/collection-compatibility.md). The tools that replay and capture them are in [tools/collection-harness](../../tools/collection-harness/README.md).

## Request fixture format

```jsonc
{
  "id": "send-event-pageview",            // equals the file name
  "surface": "send.event",                 // see the surface table in the contract
  "polarity": "positive" | "negative",     // negative = invalid or unwanted input, whatever upstream does with it
  "description": "...",
  "upstream_ref": ["src/app/api/send/route.ts:33-73"],   // paths in .sources/umami-3.4.0 at ec0ff503
  "requires_pass": "collect-alias",        // optional; replay pass that must be active (default "default")
  "setup": [ { request } ],                // optional; sent first, response must be 2xx
  "request": {
    "method": "POST",
    "path": "/api/send",
    "headers": { "x-umami-cache": "{{CACHE:<website-uuid>}}" },  // null deletes a default header
    "body": { ... }                        // JSON; or "body_raw": "<exact text>"
  },
  "expected_upstream": {
    "status": 200,
    "headers": { "access-control-allow-origin": "*" },  // "<present>" and "<absent>" allowed
    "body": { "cache": "<jwt>" },          // partial match unless "body_match": "exact"
    "body_match": "exact",                 // exact = no extra keys at any object level
    "body_sha256": "...", "body_bytes": 43, "body_contains": ["..."],
    "notes": "..."
  },
  "expected_holocron": {
    "outcome": "accepted" | "duplicate" | "suppressed" | "refused" | "served" | "not_found",
    "status": 200,
    "reason": "outside_retention",         // optional machine reason
    "decision": "confirmed" | "adopted" | "proposed",
    "notes": "..."
  },
  "validated_against_upstream": true,
  "validation_reason": "..."
}
```

`decision` labels: `confirmed` restates the technical briefing or a decision Mike made (recorded in docs/decisions as Confirmed (Mike, 2026-10-03)), `adopted` is a team decision in docs/decisions 0001, 0002, 0005 or 0006, `proposed` is not decided yet. `notes` name the record and section.

`validated_against_upstream: false` marks Holocron-only outcomes that upstream cannot produce (storage failure, batch commit failure, link and pixel outages, admission budget, replay budget). The replay runner skips them.

### Body matcher tokens

`<any>`, `<string>`, `<number>`, `<boolean>`, `<uuid>`, `<jwt>`, `<null>`, `<array>`, `<object>` match by type. `<absent>` as an object value requires the key to be missing. Arrays must have the same length and match element by element. Any other value must be equal.

### Request placeholders

| Placeholder | Replaced with |
| --- | --- |
| `{{NOW_S}}`, `{{NOW_S-90000}}`, `{{NOW_S+300}}` | Current unix seconds plus the offset, as a JSON number (whole string only) |
| `{{CACHE:<website-uuid>}}` | A real cache token, obtained by one pageview for that website |
| `{{FILL:<n>}}` | `n` times `x`, inside a string |
| `{{RUN_ID}}` | A value unique per runner invocation, inside a string |
| `{"$repeat": n, "item": X}` | An array of `n` copies of `X` |

The runner sends a desktop Chrome `User-Agent` unless the fixture sets one, because isbot flags Node's default. Fixed source IDs come from `tools/collection-harness/lib.mjs` and are created by its seed step.

## Script captures

`script-captures/<scenario>.json` hold requests that the unchanged 3.4.0 tracker and recorder sent from headless Chromium, together with the upstream responses. Each file has `script` (sha256 of the served scripts), `page` (URL and script attributes), `browser`, `actions`, `captured` (method, path, collection headers, request body, response status and body), and `assertions` with their result. CORS preflights are not listed.

Normalized values:

| Placeholder | Original |
| --- | --- |
| `<CACHE_TOKEN>` | JWT cache token in bodies and `x-umami-cache` |
| `<SESSION_ID>`, `<VISIT_ID>` | `sessionId`, `visitId` in responses |
| `<UNIX_S>`, `<UNIX_MS>` | Any `timestamp` value in seconds or milliseconds |
| `<OFFSET_MS>` | rrweb `timeOffset` |
| `<METRIC>` | `lcp`, `inp`, `cls`, `fcp`, `ttfb`, `duration` |
| `<FRAGMENT_ID>` | `data.id` of `umami:rrweb-event-fragment` events |

An empty `x-umami-cache` value is real: `identify` clears the token to `""` before sending.

## Cases

| Surface | Positive | Negative |
| --- | --- | --- |
| batch | 3 | 8 |
| link | 1 | 4 |
| pixel | 1 | 2 |
| record.heatmap | 1 | 4 |
| record.replay | 4 | 12 |
| recorder.config | 2 | 3 |
| script.asset | 4 | 1 |
| send.alias | 2 | 1 |
| send.event | 12 | 36 |
| send.identify | 5 | 4 |
| send.link | 1 | 1 |
| send.performance | 2 | 4 |
| send.pixel | 1 | 1 |

### batch

| Case | Polarity | Upstream | Holocron |
| --- | --- | --- | --- |
| [batch-body-over-2mib](batch-body-over-2mib.json) | negative | 200 | refused 413 (adopted) |
| [batch-bot-user-agent](batch-bot-user-agent.json) | negative | 200 | suppressed 200 (proposed) |
| [batch-empty](batch-empty.json) | positive | 200 | accepted 200 (proposed) |
| [batch-ignore-ips](batch-ignore-ips.json) | negative | 200 | suppressed 200 (adopted) |
| [batch-item-not-object](batch-item-not-object.json) | negative | 400 | refused 400 (proposed) |
| [batch-max-500](batch-max-500.json) | positive | 200 | accepted 200 (confirmed) |
| [batch-mixed-invalid-item](batch-mixed-invalid-item.json) | negative | 200 | accepted 200 (adopted) |
| [batch-not-array](batch-not-array.json) | negative | 400 | refused 400 (proposed) |
| [batch-over-500](batch-over-500.json) | negative | 400 | refused 400 (confirmed) |
| [batch-two-items](batch-two-items.json) | positive | 200 | accepted 200 (confirmed) |
| [holocron-batch-commit-failure](holocron-batch-commit-failure.json) | negative | n/a | refused 503 (adopted), not replayable |

### link

| Case | Polarity | Upstream | Holocron |
| --- | --- | --- | --- |
| [holocron-link-storage-unavailable](holocron-link-storage-unavailable.json) | negative | 307 | refused 307 (adopted), not replayable |
| [link-bot-user-agent](link-bot-user-agent.json) | negative | 307 | suppressed 307 (adopted) |
| [link-ignore-ips](link-ignore-ips.json) | negative | 307 | suppressed 307 (adopted) |
| [link-redirect](link-redirect.json) | positive | 307 | accepted 307 (adopted) |
| [link-unknown-slug](link-unknown-slug.json) | negative | 404 | refused 404 (adopted) |

### pixel

| Case | Polarity | Upstream | Holocron |
| --- | --- | --- | --- |
| [holocron-pixel-storage-unavailable](holocron-pixel-storage-unavailable.json) | negative | 200 | refused 200 (adopted), not replayable |
| [pixel-gif](pixel-gif.json) | positive | 200 | accepted 200 (adopted) |
| [pixel-unknown-slug](pixel-unknown-slug.json) | negative | 404 | refused 404 (adopted) |

### record.heatmap

| Case | Polarity | Upstream | Holocron |
| --- | --- | --- | --- |
| [record-heatmap-click-scroll](record-heatmap-click-scroll.json) | positive | 200 | accepted 200 (confirmed) |
| [record-heatmap-disabled](record-heatmap-disabled.json) | negative | 200 | suppressed 200 (confirmed) |
| [record-heatmap-missing-url](record-heatmap-missing-url.json) | negative | 400 | refused 400 (proposed) |
| [record-heatmap-unknown-event-type](record-heatmap-unknown-event-type.json) | negative | 400 | refused 400 (proposed) |
| [record-unknown-type](record-unknown-type.json) | negative | 400 | refused 400 (proposed) |

### record.replay

| Case | Polarity | Upstream | Holocron |
| --- | --- | --- | --- |
| [holocron-record-budget-exceeded](holocron-record-budget-exceeded.json) | negative | n/a | suppressed 200 (proposed), not replayable |
| [record-bot-user-agent](record-bot-user-agent.json) | negative | 200 | suppressed 200 (confirmed) |
| [record-cross-site-token](record-cross-site-token.json) | negative | 200 | refused 400 (adopted) |
| [record-empty-events](record-empty-events.json) | positive | 200 | accepted 200 (proposed) |
| [record-ignore-ips](record-ignore-ips.json) | negative | 403 | suppressed 403 (adopted) |
| [record-invalid-token](record-invalid-token.json) | negative | 400 | refused 400 (confirmed) |
| [record-missing-token](record-missing-token.json) | negative | 400 | refused 400 (confirmed) |
| [record-over-200-events](record-over-200-events.json) | negative | 400 | refused 400 (confirmed) |
| [record-oversize](record-oversize.json) | negative | 413 | refused 413 (confirmed) |
| [record-preflight](record-preflight.json) | positive | 204 | served 204 (confirmed) |
| [record-recorder-disabled](record-recorder-disabled.json) | negative | 200 | suppressed 200 (confirmed) |
| [record-replay-chunk](record-replay-chunk.json) | positive | 200 | accepted 200 (confirmed) |
| [record-replay-disabled](record-replay-disabled.json) | negative | 200 | suppressed 200 (confirmed) |
| [record-replay-fragment](record-replay-fragment.json) | positive | 200 | accepted 200 (confirmed) |
| [record-unknown-website](record-unknown-website.json) | negative | 400 | refused 400 (proposed) |
| [record-website-not-uuid](record-website-not-uuid.json) | negative | 400 | refused 400 (proposed) |

### recorder.config

| Case | Polarity | Upstream | Holocron |
| --- | --- | --- | --- |
| [recorder-config-disabled-site](recorder-config-disabled-site.json) | negative | 200 | served 200 (confirmed) |
| [recorder-config-enabled](recorder-config-enabled.json) | positive | 200 | served 200 (confirmed) |
| [recorder-config-not-uuid](recorder-config-not-uuid.json) | negative | 200 | served 200 (proposed) |
| [recorder-config-preflight](recorder-config-preflight.json) | positive | 204 | served 204 (confirmed) |
| [recorder-config-unknown-site](recorder-config-unknown-site.json) | negative | 200 | served 200 (proposed) |

### script.asset

| Case | Polarity | Upstream | Holocron |
| --- | --- | --- | --- |
| [script-alias-nested-path](script-alias-nested-path.json) | positive | 200 | served 200 (proposed) |
| [script-alias-tracker-name](script-alias-tracker-name.json) | positive | 200 | served 200 (confirmed) |
| [script-recorder-get](script-recorder-get.json) | positive | 200 | served 200 (confirmed) |
| [script-tracker-get](script-tracker-get.json) | positive | 200 | served 200 (confirmed) |
| [script-unknown-name](script-unknown-name.json) | negative | 404 | not_found 404 (proposed) |

### send.alias

| Case | Polarity | Upstream | Holocron |
| --- | --- | --- | --- |
| [send-alias-collect-endpoint](send-alias-collect-endpoint.json) | positive | 200 | accepted 200 (confirmed) |
| [send-alias-script-rewritten](send-alias-script-rewritten.json) | positive | 200 | served 200 (proposed) |
| [send-alias-unconfigured](send-alias-unconfigured.json) | negative | 404 | not_found 404 (proposed) |

### send.event

| Case | Polarity | Upstream | Holocron |
| --- | --- | --- | --- |
| [holocron-send-admission-budget-full](holocron-send-admission-budget-full.json) | negative | n/a | refused 429 (adopted), not replayable |
| [holocron-send-storage-unavailable](holocron-send-storage-unavailable.json) | negative | 500 | refused 503 (adopted), not replayable |
| [send-event-body-over-64kib](send-event-body-over-64kib.json) | negative | 200 | refused 413 (adopted) |
| [send-event-body-under-64kib](send-event-body-under-64kib.json) | positive | 200 | accepted 200 (adopted) |
| [send-event-bot-payload-user-agent](send-event-bot-payload-user-agent.json) | negative | 200 | accepted 200 (adopted) |
| [send-event-bot-user-agent](send-event-bot-user-agent.json) | negative | 200 | suppressed 200 (confirmed) |
| [send-event-consent-in-data](send-event-consent-in-data.json) | negative | 200 | accepted 200 (confirmed) |
| [send-event-cross-site-cache-token](send-event-cross-site-cache-token.json) | negative | 200 | refused 400 (adopted) |
| [send-event-custom-with-data](send-event-custom-with-data.json) | positive | 200 | accepted 200 (confirmed) |
| [send-event-data-array](send-event-data-array.json) | negative | 400 | refused 400 (proposed) |
| [send-event-empty-body](send-event-empty-body.json) | negative | 400 | refused 400 (proposed) |
| [send-event-eventid-duplicate-different-payload](send-event-eventid-duplicate-different-payload.json) | negative | 200 | duplicate 200 (adopted) |
| [send-event-eventid-duplicate-same-payload](send-event-eventid-duplicate-same-payload.json) | negative | 200 | duplicate 200 (adopted) |
| [send-event-eventid-first](send-event-eventid-first.json) | positive | 200 | accepted 200 (adopted) |
| [send-event-eventid-too-long](send-event-eventid-too-long.json) | negative | 200 | refused 400 (proposed) |
| [send-event-explicit-timestamp](send-event-explicit-timestamp.json) | positive | 200 | accepted 200 (confirmed) |
| [send-event-ignore-ips-trusted-proxy](send-event-ignore-ips-trusted-proxy.json) | negative | 403 | suppressed 403 (adopted) |
| [send-event-ignored-ip-header](send-event-ignored-ip-header.json) | negative | 403 | accepted 200 (adopted) |
| [send-event-ignored-ip-payload](send-event-ignored-ip-payload.json) | negative | 403 | accepted 200 (adopted) |
| [send-event-invalid-cache-token](send-event-invalid-cache-token.json) | negative | 200 | accepted 200 (adopted) |
| [send-event-invalid-type](send-event-invalid-type.json) | negative | 400 | refused 400 (proposed) |
| [send-event-malformed-json](send-event-malformed-json.json) | negative | 400 | refused 400 (proposed) |
| [send-event-missing-source](send-event-missing-source.json) | negative | 400 | refused 400 (confirmed) |
| [send-event-name-formula-trigger](send-event-name-formula-trigger.json) | negative | 400 | refused 400 (proposed) |
| [send-event-name-over-50](send-event-name-over-50.json) | positive | 200 | accepted 200 (proposed) |
| [send-event-oversize-body](send-event-oversize-body.json) | negative | 200 | refused 413 (adopted) |
| [send-event-pageview](send-event-pageview.json) | positive | 200 | accepted 200 (confirmed) |
| [send-event-properties-over-50](send-event-properties-over-50.json) | negative | 200 | accepted 200 (adopted) |
| [send-event-text-plain-content-type](send-event-text-plain-content-type.json) | positive | 200 | accepted 200 (proposed) |
| [send-event-timestamp-boundary-minus-24h-1s](send-event-timestamp-boundary-minus-24h-1s.json) | negative | 200 | accepted 200 (confirmed) |
| [send-event-timestamp-boundary-minus-24h](send-event-timestamp-boundary-minus-24h.json) | positive | 200 | accepted 200 (confirmed) |
| [send-event-timestamp-boundary-plus-5min-1s](send-event-timestamp-boundary-plus-5min-1s.json) | negative | 200 | accepted 200 (confirmed) |
| [send-event-timestamp-boundary-plus-5min](send-event-timestamp-boundary-plus-5min.json) | positive | 200 | accepted 200 (confirmed) |
| [send-event-timestamp-future-10min](send-event-timestamp-future-10min.json) | negative | 200 | accepted 200 (confirmed) |
| [send-event-timestamp-i64-overflow](send-event-timestamp-i64-overflow.json) | negative | 400 | refused 400 (adopted) |
| [send-event-timestamp-keyless-day](send-event-timestamp-keyless-day.json) | negative | 200 | accepted 200 (adopted) |
| [send-event-timestamp-late-25h](send-event-timestamp-late-25h.json) | negative | 200 | accepted 200 (confirmed) |
| [send-event-timestamp-milliseconds](send-event-timestamp-milliseconds.json) | negative | 200 | accepted 200 (confirmed) |
| [send-event-timestamp-non-integer](send-event-timestamp-non-integer.json) | negative | 400 | refused 400 (proposed) |
| [send-event-timestamp-not-numeric](send-event-timestamp-not-numeric.json) | negative | 400 | refused 400 (proposed) |
| [send-event-timestamp-outside-retention](send-event-timestamp-outside-retention.json) | negative | 200 | suppressed 200 (adopted) |
| [send-event-unknown-fields-stripped](send-event-unknown-fields-stripped.json) | positive | 200 | accepted 200 (proposed) |
| [send-event-url-invalid](send-event-url-invalid.json) | negative | 400 | refused 400 (proposed) |
| [send-event-website-and-link](send-event-website-and-link.json) | negative | 400 | refused 400 (confirmed) |
| [send-event-website-not-uuid](send-event-website-not-uuid.json) | negative | 400 | refused 400 (proposed) |
| [send-event-website-unknown](send-event-website-unknown.json) | negative | 400 | refused 400 (proposed) |
| [send-event-with-cache-token](send-event-with-cache-token.json) | positive | 200 | accepted 200 (confirmed) |
| [send-preflight](send-preflight.json) | positive | 204 | served 204 (confirmed) |

### send.identify

| Case | Polarity | Upstream | Holocron |
| --- | --- | --- | --- |
| [send-identify-consent-invalid-value](send-identify-consent-invalid-value.json) | negative | 200 | accepted 200 (adopted) |
| [send-identify-consent-none](send-identify-consent-none.json) | positive | 200 | accepted 200 (confirmed) |
| [send-identify-consent-persistent](send-identify-consent-persistent.json) | positive | 200 | accepted 200 (confirmed) |
| [send-identify-consent-without-id](send-identify-consent-without-id.json) | negative | 200 | accepted 200 (adopted) |
| [send-identify-data-not-object](send-identify-data-not-object.json) | negative | 400 | refused 400 (proposed) |
| [send-identify-data-only](send-identify-data-only.json) | positive | 200 | accepted 200 (confirmed) |
| [send-identify-id-and-data](send-identify-id-and-data.json) | positive | 200 | accepted 200 (confirmed) |
| [send-identify-id-over-50](send-identify-id-over-50.json) | positive | 200 | accepted 200 (adopted) |
| [send-identify-link-source](send-identify-link-source.json) | negative | 200 | refused 400 (proposed) |

### send.link

| Case | Polarity | Upstream | Holocron |
| --- | --- | --- | --- |
| [send-link-source](send-link-source.json) | positive | 200 | accepted 200 (confirmed) |
| [send-link-unknown-id](send-link-unknown-id.json) | negative | 200 | refused 400 (adopted) |

### send.performance

| Case | Polarity | Upstream | Holocron |
| --- | --- | --- | --- |
| [send-performance-all-vitals](send-performance-all-vitals.json) | positive | 200 | accepted 200 (confirmed) |
| [send-performance-boundary-max](send-performance-boundary-max.json) | positive | 200 | accepted 200 (proposed) |
| [send-performance-cls-over-max](send-performance-cls-over-max.json) | negative | 400 | refused 400 (proposed) |
| [send-performance-lcp-over-max](send-performance-lcp-over-max.json) | negative | 400 | refused 400 (proposed) |
| [send-performance-negative](send-performance-negative.json) | negative | 400 | refused 400 (proposed) |
| [send-performance-string-metric](send-performance-string-metric.json) | negative | 400 | refused 400 (proposed) |

### send.pixel

| Case | Polarity | Upstream | Holocron |
| --- | --- | --- | --- |
| [send-pixel-source](send-pixel-source.json) | positive | 200 | accepted 200 (confirmed) |
| [send-pixel-unknown-id](send-pixel-unknown-id.json) | negative | 200 | refused 400 (adopted) |

### script captures

| Capture | Requests | Description |
| --- | --- | --- |
| [auto-track-off](script-captures/auto-track-off.json) | 1 | data-auto-track="false" sends no pageview and does not hook clicks or history; umami.track still works. |
| [before-send-cancel](script-captures/before-send-cancel.json) | 0 | data-before-send names a global function; returning null cancels the request. |
| [before-send-consent-extension](script-captures/before-send-consent-extension.json) | 2 | Holocron consent transport (docs/decisions/0001): data-before-send adds payload.consent = "persistent" to identify calls. Upstream accepts and strips the key. |
| [bot-user-agent](script-captures/bot-user-agent.json) | 3 | Default headless Chromium UA ("HeadlessChrome") is flagged by isbot. Upstream answers {beep:"boop"}, the tracker keeps no token, and the recorder never starts because it waits for a token. |
| [custom-event-data-attribute](script-captures/custom-event-data-attribute.json) | 2 | Click on an element with data-umami-event sends a custom event; data-umami-event-* attributes become string data. |
| [distinct-id-attribute](script-captures/distinct-id-attribute.json) | 2 | data-distinct-id triggers identify immediately at script start, before the pageview. |
| [do-not-track-respected](script-captures/do-not-track-respected.json) | 0 | data-do-not-track="true" with navigator.doNotTrack = "1" disables all tracking. |
| [do-not-track-without-attribute](script-captures/do-not-track-without-attribute.json) | 1 | navigator.doNotTrack = "1" is ignored unless the site opts in with data-do-not-track. |
| [domains-match](script-captures/domains-match.json) | 1 | data-domains entries are trimmed and matched exactly against location.hostname. |
| [domains-mismatch](script-captures/domains-mismatch.json) | 0 | data-domains not containing location.hostname disables tracking. |
| [exclude-search-and-hash](script-captures/exclude-search-and-hash.json) | 1 | data-exclude-search and data-exclude-hash strip query and fragment from url (and referrer) before sending. |
| [identify](script-captures/identify.json) | 3 | umami.identify(id, data) sends type identify with an empty x-umami-cache header; later events carry payload.id. |
| [local-storage-disabled](script-captures/local-storage-disabled.json) | 0 | localStorage "umami.disabled" set disables tracking. |
| [pageview-on-load](script-captures/pageview-on-load.json) | 1 | Default tracker on page load sends one pageview without a cache header and stores the returned token. |
| [performance](script-captures/performance.json) | 2 | data-performance="true" sends one performance payload after the 10 s timer, with web vitals and duration. |
| [recorder-disabled-site](script-captures/recorder-disabled-site.json) | 2 | Recorder on a site without recorder enabled fetches configuration, gets enabled:false, and sends nothing. |
| [recorder-replay-and-heatmap](script-captures/recorder-replay-and-heatmap.json) | 6 | Recorder on a site with replay and heatmaps at sample rate 1 and strict masking: configuration fetch, replay chunks with the tracker cache token, heatmap click and scroll payloads. Masked text and input values never leave the browser. |
| [spa-navigation](script-captures/spa-navigation.json) | 2 | pushState to a new URL sends a pageview after 300 ms with the previous URL as a path-only referrer and the cache token. replaceState to the same URL and popstate send nothing. |
| [tag](script-captures/tag.json) | 2 | data-tag is sent as payload.tag on every event. |
| [track-api](script-captures/track-api.json) | 4 | umami.track with a name and data, with a full payload object, and with a function. |
