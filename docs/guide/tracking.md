# Tracking

Holocron serves the unchanged Umami 3.4.0 tracker and recorder, byte for byte. Everything you know from Umami's tracker works the same way: the script tag, its data attributes, `umami.track` and `umami.identify`. What changes is what the server does with the events.

## The script tag

Each site's settings show its tracking code under Tracking. It looks like this:

```html
<script defer src="https://analytics.example.com/script.js"
  data-website-id="94db1cb1-74f4-4a40-ad6c-962362670409"></script>
```

Put it in the `<head>` of every page you want to track. The tracker posts to `/api/send` on the host it was loaded from. Only roles that can configure a site see its tracking code.

## Data attributes

These are the Umami 3.4.0 tracker's attributes. Holocron reads none of them; they change what the browser sends.

| Attribute | Effect |
| --- | --- |
| `data-website-id` | The site ID. Required |
| `data-host-url` | Send events to this host instead of the one the script came from |
| `data-auto-track="false"` | No automatic pageviews and no `data-umami-event` clicks. Track everything with `umami.track` |
| `data-auto-pageview="false"` | No automatic pageviews. Click events from `data-umami-event` still work |
| `data-domains` | Comma-separated host names the tracker runs on. Elsewhere, for example on staging, it sends nothing |
| `data-tag` | A tag on every event, to tell versions or variants apart |
| `data-exclude-search="true"` | Drop the query string from URLs |
| `data-exclude-hash="true"` | Drop the hash from URLs |
| `data-do-not-track="true"` | Send nothing when the browser has Do Not Track on |
| `data-before-send` | Name of a global function that gets `(type, payload)` and returns the payload to send, or a falsy value to send nothing |
| `data-distinct-id` | Calls `identify` with this ID when the script loads |
| `data-performance="true"` | Send Core Web Vitals for the Performance report |
| `data-fetch-credentials` | The `credentials` mode of the tracker's requests. Default `omit` |

## Custom events

Add `data-umami-event` to an element to track clicks on it. More `data-umami-event-*` attributes become event properties:

```html
<button data-umami-event="Signup" data-umami-event-plan="pro">Sign up</button>
```

From JavaScript:

```js
umami.track('Signup', { plan: 'pro', seats: 5 })
```

Properties are stored typed: numbers stay numbers, so a filter like `seats > 3` works. In Explore you can filter, split and compare by any property.

## Revenue

Send `revenue` and `currency` as properties of an event:

```js
umami.track('Checkout', { revenue: 49.9, currency: 'EUR' })
```

The Revenue report keeps every currency apart. Holocron does not convert between them.

## Identify

`umami.identify` attaches properties to the visitor:

```js
umami.identify('user-123', { plan: 'pro' })
```

By default an ID only lasts the day, like an anonymous visitor. One ID on one day is one visitor, across devices. The raw ID is never stored. The next day the same ID is a new visitor.

## Visitors and privacy

Anonymous visitors are counted with a keyed pseudonym built from the IP address and the browser's user agent. The key changes every day per site and is deleted about 25 hours after the day ends. Holocron sets no cookie and stores neither the address nor the user agent.

While the key exists, an operator holding the database could check whether a known IP and browser produced a given pseudonym. Afterwards nobody can. Pseudonyms are not people: shared networks merge visitors and device switches split them. That is why multi-day reports say visitor-days.

Known bots are filtered and never stored.

## Consented persistent profiles

Retention and cohort reports follow people across days. That needs a persistent profile, which Holocron only creates for visitors who consented:

1. In the site's settings under Identity, turn on "Link consented visitors to persistent profiles".
2. After a visitor consents, add `consent: "persistent"` to the payload in your `data-before-send` function, then call `identify`:

   ```html
   <script>
     function holocronBeforeSend(type, payload) {
       if (window.analyticsConsent === true) payload.consent = 'persistent'
       return payload
     }
   </script>
   <script defer src="https://analytics.example.com/script.js"
     data-website-id="..." data-before-send="holocronBeforeSend"></script>
   ```

   ```js
   umami.identify('user-123')
   ```

3. When the visitor withdraws consent, send `consent: "none"` with the same ID.

Events before the grant stay anonymous. Withdrawing stops linking new events. Turning the setting off revokes every consent on the site. To delete one person's history, use [`holocron erase-profile`](operations.md#erase-a-visitor-profile).

Consent is what your page says it is: Holocron does not verify it. Keeping a record of consent is your job as the site operator. An unchanged Umami server ignores the extra `consent` key, so the same snippet works on both.

## Replays and heatmaps

Recording is off for every site until someone turns it on. In the site's settings under Recording:

- Turn on replays, heatmaps, or both.
- Set the share of visits recorded, 1 to 100 %. The default is 10 % for each.
- Set the longest replay, 1 to 60 minutes. The default is 5.
- List elements to leave out as CSS selectors, up to 20. The recorder leaves them out in the browser, before anything is sent.
- List pages to leave out as path patterns with `*`, such as `/account/*`, up to 50.

Then add the recorder next to the tracker:

```html
<script defer src="https://analytics.example.com/recorder.js"
  data-website-id="94db1cb1-74f4-4a40-ad6c-962362670409"></script>
```

What to know about privacy:

- Masking is always strict. The recorder replaces text and input values before they leave the browser. Holocron can't prove what an arbitrary client masked, so only the unchanged Umami 3.4.0 recorder is supported.
- The recorder can't skip a page. Page exclusions run on the server: a heatmap event on an excluded path is dropped, and a replay that reaches one is deleted with every later part. The visitor's browser has already sent that data.
- Replays play in a separate frame that allows no network requests, and Holocron strips scripts and event handlers from captured pages.

Raw replays are kept 14 days, heatmaps 90 days. Saved replays are kept until someone unsaves them, within a fifth of the replay cap. Replays share a 25 GiB cap of compressed storage (`HOLOCRON_RECORDING_CAP_MIB`). At the cap new replay data is refused while every other event is still collected.

## Server-side events

Your own servers can send to `/api/send` and `/api/batch` with a collection key. Create one in the site's settings under Tracking, and send it as `Authorization: Bearer <key>`. A key works for its site only.

With a key, Holocron takes the visitor from `payload.ip` and `payload.userAgent` instead of the connection. Without a key those two fields are ignored, so a browser can't pick its own pseudonym.

## Collection settings

Set these on `holocron serve`. See [Configuration](configuration.md#environment-variables).

| Variable | Umami equivalent | Effect |
| --- | --- | --- |
| `HOLOCRON_COLLECT_IGNORE_IPS` | `IGNORE_IP` | Events from these addresses or CIDRs are not stored. The tracker gets a 403, as with Umami |
| `HOLOCRON_TRACKER_SCRIPT_NAME` | `TRACKER_SCRIPT_NAME` | Extra paths the tracker is served under, such as `stats.js`. `/script.js` keeps working |
| `HOLOCRON_COLLECT_API_ENDPOINT` | `COLLECT_API_ENDPOINT` | An extra path for `/api/send`. The served tracker posts there |

The visitor address used by `HOLOCRON_COLLECT_IGNORE_IPS` follows the [proxy rules](configuration.md#proxies-and-visitor-addresses).

## Lost events

Holocron answers the tracker only after an event is on disk. When it can't store events, because the disk reserve is reached or storage fails, `/api/send` answers 503, and 429 when too much is queued at once.

The Umami tracker never retries. Every 429 or 503 loses that event. `holocron diagnostics` counts these refusals since the server started, under `collection.outcomes`. Server status in the administration area and `/health/ready` show why collection refuses. See [Operations](operations.md#health-endpoints).

## Links and pixels

A link is a short address on your Holocron server, `/q/<slug>`, that counts a visit and redirects to a page. A pixel, `/p/<slug>`, counts a view and answers with a 1x1 image, for emails and pages without the tracker:

```html
<img src="https://analytics.example.com/p/newsletter" alt="" width="1" height="1">
```

Manage them in a workspace under Links and pixels. They belong to the workspace, not to a site:

- Members and above add them and change their name, slug and, for links, the URL. Admins and owners delete them. Everyone in the workspace reads their reports.
- A slug is 1 to 100 letters, digits, `-` or `_`. Leave it empty and Holocron picks 8 random characters. Links and pixels each need their own slugs, so a link and a pixel may share one.
- A link's URL must be an `http` or `https` address without a user name or password.
- Changing a slug takes effect at once. The old address stops working and someone else can take it. Holocron doesn't redirect old slugs, so change one only when nobody uses the old address any more.
- Deleting asks for your password and erases every hit. The address stops working at once.

A link's or pixel's report shows its hits as events and its visitors per UTC day, with referrers, countries, browsers and devices. Query strings on the address, such as `/q/spring?utm_source=newsletter`, are kept with the hit. Hits from bots are not stored. When storage is down, the redirect or image is still answered and the hit is counted as lost; the report says how many were lost.

Links and pixels keep their hits as long as the installation default retention says. See [Retention](operations.md#retention).
