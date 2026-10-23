// Runs the real Umami 3.4.0 tracker and recorder, served by the upstream container, in headless
// Chromium against a local test page. Captures every collection request and the upstream response,
// normalizes volatile values, checks scenario assertions, and writes
// fixtures/collection/script-captures/<scenario>.json.
//
// Usage: node tools/collection-harness/capture.mjs [--only <scenario-prefix>] [--out <dir>]
// --out writes the captures elsewhere, for example when UMAMI_URL points at Holocron.
import { createHash } from 'node:crypto';
import { mkdir, writeFile } from 'node:fs/promises';
import { createServer } from 'node:http';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';
import { BASE_URL, BROWSER_UA, IDS } from './lib.mjs';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const outDir = process.argv.includes('--out')
  ? path.resolve(process.argv[process.argv.indexOf('--out') + 1])
  : path.join(root, 'fixtures/collection/script-captures');
const PAGE_PORT = 33401;
const PAGE_ORIGIN = `http://127.0.0.1:${PAGE_PORT}`;
const only = process.argv.includes('--only') ? process.argv[process.argv.indexOf('--only') + 1] : undefined;

const TRACKER = 'src/tracker/index.ts';
const RECORDER = 'src/recorder/index.js';
const SECRET_TEXT = 'Secret customer text 4711';
const SECRET_INPUT = 'hunter2-typed-value';
/** Attribute values are not text, so masking keeps them; only the block selector removes them. */
const SECRET_ATTRIBUTE = 'DE89-3704-0044-0532';

/* Test page */

let currentPage = { tracker: {}, recorder: null, body: '' };

const attrs = obj =>
  Object.entries(obj)
    .map(([k, v]) => ` ${k}="${v}"`)
    .join('');

function html({ tracker, recorder, body, head = '' }) {
  return `<!doctype html><html><head><meta charset="utf-8"><title>Capture page</title>${head}
<script defer src="${BASE_URL}/script.js"${attrs(tracker)}></script>
${recorder ? `<script defer src="${BASE_URL}/recorder.js"${attrs(recorder)}></script>` : ''}
</head><body>${body}</body></html>`;
}

const server = createServer((req, res) => {
  res.writeHead(200, { 'content-type': 'text/html; charset=utf-8', 'cache-control': 'no-store' });
  res.end(html(currentPage));
});
await new Promise(r => server.listen(PAGE_PORT, '127.0.0.1', r));

/* Normalization */

const JWT = /^[\w-]+\.[\w-]+\.[\w-]+$/;
const METRICS = new Set(['lcp', 'inp', 'cls', 'fcp', 'ttfb', 'duration']);

function normalize(value, key) {
  if (Array.isArray(value)) return value.map(v => normalize(v));
  if (value && typeof value === 'object') {
    const out = Object.fromEntries(Object.entries(value).map(([k, v]) => [k, normalize(v, k)]));
    if (out.type === 'umami:rrweb-event-fragment' && out.data) out.data = { ...out.data, id: '<FRAGMENT_ID>' };
    return out;
  }
  if (typeof value === 'string') {
    if (JWT.test(value) && value.length > 40) return '<CACHE_TOKEN>';
    if (key === 'sessionId') return '<SESSION_ID>';
    if (key === 'visitId') return '<VISIT_ID>';
    return value;
  }
  if (typeof value === 'number') {
    if (key === 'timestamp') return value > 1e12 ? '<UNIX_MS>' : '<UNIX_S>';
    if (key === 'timeOffset') return '<OFFSET_MS>';
    if (METRICS.has(key)) return '<METRIC>';
  }
  return value;
}

const KEPT_HEADERS = ['content-type', 'x-umami-cache', 'x-umami-website-id', 'x-umami-hostname'];
const KEPT_RESPONSE_HEADERS = ['content-type', 'location'];
const collected = path => path.startsWith('/api/') || path.startsWith('/q/') || path.startsWith('/p/');

async function record(request) {
  const url = new URL(request.url());
  const headers = await request.allHeaders();
  const response = await request.response();
  let body = request.postData();
  try {
    body = body && JSON.parse(body);
  } catch {
    /* keep text */
  }
  let responseBody = null;
  try {
    responseBody = await response?.json();
  } catch {
    responseBody = null;
  }
  return {
    method: request.method(),
    path: url.pathname,
    request_headers: Object.fromEntries(
      KEPT_HEADERS.filter(h => h in headers).map(h => [h, normalize(headers[h], h === 'x-umami-cache' ? undefined : h)]),
    ),
    request_body: normalize(body),
    raw_body: request.postData() ?? '',
    response: {
      status: response?.status() ?? null,
      ...(!url.pathname.startsWith('/api/') && {
        headers: Object.fromEntries(KEPT_RESPONSE_HEADERS.flatMap(h => (response?.headers()[h] ? [[h, response.headers()[h]]] : []))),
      }),
      body: normalize(responseBody),
    },
  };
}

/* Scenarios */

const plain = { 'data-website-id': IDS.websitePlain };
const sends = c => c.filter(r => r.path === '/api/send');
const records = c => c.filter(r => r.path === '/api/record');

const scenarios = [
  {
    id: 'pageview-on-load',
    description: 'Default tracker on page load sends one pageview without a cache header and stores the returned token.',
    upstream_ref: [`${TRACKER}:283-293`, `${TRACKER}:384-420`, `${TRACKER}:422-430`, `${TRACKER}:652-658`],
    page: { tracker: plain, path: '/fixture?utm_source=newsletter#intro' },
    async act() {},
    wait: 1,
    assert: c => [
      ['exactly one /api/send request', sends(c).length === 1],
      ['type is event with no name', sends(c)[0]?.request_body.type === 'event' && !('name' in sends(c)[0].request_body.payload)],
      ['first request has no x-umami-cache header', !('x-umami-cache' in (sends(c)[0]?.request_headers ?? {}))],
      ['x-umami-website-id and x-umami-hostname headers are sent', !!sends(c)[0]?.request_headers['x-umami-website-id'] && !!sends(c)[0]?.request_headers['x-umami-hostname']],
      ['url keeps query and hash', sends(c)[0]?.request_body.payload.url === `${PAGE_ORIGIN}/fixture?utm_source=newsletter#intro`],
      ['upstream returned a cache token', sends(c)[0]?.response.body?.cache === '<CACHE_TOKEN>'],
    ],
  },
  {
    id: 'spa-navigation',
    description: 'pushState to a new URL sends a pageview after 300 ms with the previous URL as a path-only referrer and the cache token. replaceState to the same URL and popstate send nothing.',
    upstream_ref: [`${TRACKER}:280-281`, `${TRACKER}:302-333`],
    page: { tracker: plain, path: '/fixture' },
    async act(page) {
      await page.evaluate(() => history.pushState({}, '', '/next?step=2'));
      await page.waitForTimeout(800);
      await page.evaluate(() => history.replaceState({}, '', '/next?step=2'));
      await page.evaluate(() => history.back());
      await page.waitForTimeout(800);
    },
    wait: 2,
    assert: c => [
      ['two pageviews in total', sends(c).length === 2],
      ['second pageview url is /next?step=2', sends(c)[1]?.request_body.payload.url === `${PAGE_ORIGIN}/next?step=2`],
      ['second pageview referrer is the previous path without origin', sends(c)[1]?.request_body.payload.referrer === '/fixture'],
      ['second request carries the cache token', sends(c)[1]?.request_headers['x-umami-cache'] === '<CACHE_TOKEN>'],
    ],
  },
  {
    id: 'custom-event-data-attribute',
    description: 'Click on an element with data-umami-event sends a custom event; data-umami-event-* attributes become string data.',
    upstream_ref: [`${TRACKER}:260-261`, `${TRACKER}:335-373`],
    page: { tracker: plain, body: '<button id="b" data-umami-event="signup-button" data-umami-event-plan="pro" data-umami-event-seats="3">Sign up</button>' },
    async act(page) {
      await page.click('#b');
    },
    wait: 2,
    assert: c => [
      ['pageview then custom event', sends(c).length === 2],
      ['event name from attribute', sends(c)[1]?.request_body.payload.name === 'signup-button'],
      ['attribute data are strings', JSON.stringify(sends(c)[1]?.request_body.payload.data) === JSON.stringify({ plan: 'pro', seats: '3' })],
    ],
  },
  {
    id: 'track-api',
    description: 'umami.track with a name and data, with a full payload object, and with a function.',
    upstream_ref: [`${TRACKER}:432-440`],
    page: { tracker: plain },
    async act(page) {
      await page.evaluate(async () => {
        await window.umami.track('signup', { plan: 'pro', seats: 3, trial: false });
        await window.umami.track({ website: document.currentScript?.dataset.websiteId ?? '5e1a0000-0000-4000-8000-000000000001', url: '/custom', title: 'Custom' });
        await window.umami.track(props => ({ ...props, name: 'from-function', data: { n: 1 } }));
      });
    },
    wait: 4,
    assert: c => [
      ['four sends (pageview + three track calls)', sends(c).length === 4],
      ['typed data preserved for track(name, data)', JSON.stringify(sends(c)[1]?.request_body.payload.data) === JSON.stringify({ plan: 'pro', seats: 3, trial: false })],
      ['object form sends only the given fields', JSON.stringify(Object.keys(sends(c)[2]?.request_body.payload ?? {})) === JSON.stringify(['website', 'url', 'title'])],
      ['function form receives the default payload', sends(c)[3]?.request_body.payload.name === 'from-function' && !!sends(c)[3]?.request_body.payload.screen],
    ],
  },
  {
    id: 'identify',
    description: 'umami.identify(id, data) sends type identify with an empty x-umami-cache header; later events carry payload.id.',
    upstream_ref: [`${TRACKER}:442-460`, `${TRACKER}:292`, `${TRACKER}:406`],
    page: { tracker: plain },
    async act(page) {
      await page.waitForTimeout(300);
      await page.evaluate(async () => {
        await window.umami.identify('user-123', { plan: 'pro' });
        await window.umami.track('after-identify');
      });
    },
    wait: 3,
    assert: c => [
      ['identify request present', sends(c)[1]?.request_body.type === 'identify'],
      ['identify sends an empty cache header', sends(c)[1]?.request_headers['x-umami-cache'] === ''],
      ['identify payload has id and data', sends(c)[1]?.request_body.payload.id === 'user-123' && sends(c)[1]?.request_body.payload.data?.plan === 'pro'],
      ['next event carries id', sends(c)[2]?.request_body.payload.id === 'user-123'],
    ],
  },
  {
    id: 'distinct-id-attribute',
    description: 'data-distinct-id triggers identify immediately at script start, before the pageview.',
    upstream_ref: [`${TRACKER}:244`, `${TRACKER}:648-650`],
    page: { tracker: { ...plain, 'data-distinct-id': 'attr-user' } },
    async act() {},
    wait: 2,
    assert: c => [
      ['identify first', sends(c)[0]?.request_body.type === 'identify' && sends(c)[0]?.request_body.payload.id === 'attr-user'],
      ['pageview carries id', sends(c).some(r => r.request_body.type === 'event' && r.request_body.payload.id === 'attr-user')],
    ],
  },
  {
    id: 'before-send-cancel',
    description: 'data-before-send names a global function; returning null cancels the request.',
    upstream_ref: [`${TRACKER}:243`, `${TRACKER}:387-395`],
    page: { tracker: { ...plain, 'data-before-send': 'cancelAll' }, head: '<script>window.cancelAll = () => null;</script>' },
    async act(page) {
      await page.evaluate(() => window.umami.track('cancelled'));
    },
    expectNone: true,
    assert: c => [['no /api/send request', sends(c).length === 0]],
  },
  {
    id: 'before-send-consent-extension',
    description: 'Holocron consent transport (docs/decisions/0001): data-before-send adds payload.consent = "persistent" to identify calls. Upstream accepts and strips the key.',
    upstream_ref: [`${TRACKER}:387-395`, 'src/app/api/send/route.ts:33-73'],
    page: {
      tracker: { ...plain, 'data-before-send': 'addConsent' },
      head: '<script>window.addConsent = (type, payload) => (type === "identify" ? { ...payload, consent: "persistent" } : payload);</script>',
    },
    async act(page) {
      await page.waitForTimeout(300);
      await page.evaluate(() => window.umami.identify('user-123'));
    },
    wait: 2,
    assert: c => [
      ['identify carries consent', sends(c).find(r => r.request_body.type === 'identify')?.request_body.payload.consent === 'persistent'],
      ['pageview has no consent key', !('consent' in (sends(c).find(r => r.request_body.type === 'event')?.request_body.payload ?? {}))],
      ['upstream accepted identify', sends(c).find(r => r.request_body.type === 'identify')?.response.status === 200],
    ],
  },
  {
    id: 'do-not-track-respected',
    description: 'data-do-not-track="true" with navigator.doNotTrack = "1" disables all tracking.',
    upstream_ref: [`${TRACKER}:247`, `${TRACKER}:295-298`, `${TRACKER}:377-382`],
    page: { tracker: { ...plain, 'data-do-not-track': 'true' } },
    dnt: true,
    async act(page) {
      await page.evaluate(() => window.umami.track('ignored'));
    },
    expectNone: true,
    assert: c => [['no /api/send request', sends(c).length === 0]],
  },
  {
    id: 'do-not-track-without-attribute',
    description: 'navigator.doNotTrack = "1" is ignored unless the site opts in with data-do-not-track.',
    upstream_ref: [`${TRACKER}:247`, `${TRACKER}:382`],
    page: { tracker: plain },
    dnt: true,
    async act() {},
    wait: 1,
    assert: c => [['pageview sent', sends(c).length === 1]],
  },
  {
    id: 'domains-mismatch',
    description: 'data-domains not containing location.hostname disables tracking.',
    upstream_ref: [`${TRACKER}:250`, `${TRACKER}:255`, `${TRACKER}:381`],
    page: { tracker: { ...plain, 'data-domains': 'example.com, www.example.com' } },
    async act() {},
    expectNone: true,
    assert: c => [['no /api/send request', sends(c).length === 0]],
  },
  {
    id: 'domains-match',
    description: 'data-domains entries are trimmed and matched exactly against location.hostname.',
    upstream_ref: [`${TRACKER}:255`, `${TRACKER}:381`],
    page: { tracker: { ...plain, 'data-domains': 'example.com, 127.0.0.1' } },
    async act() {},
    wait: 1,
    assert: c => [['pageview sent', sends(c).length === 1]],
  },
  {
    id: 'exclude-search-and-hash',
    description: 'data-exclude-search and data-exclude-hash strip query and fragment from url (and referrer) before sending.',
    upstream_ref: [`${TRACKER}:248-249`, `${TRACKER}:266-276`],
    page: { tracker: { ...plain, 'data-exclude-search': 'true', 'data-exclude-hash': 'true' }, path: '/fixture?utm_source=newsletter#intro' },
    async act() {},
    wait: 1,
    assert: c => [['url has no query or hash', sends(c)[0]?.request_body.payload.url === `${PAGE_ORIGIN}/fixture`]],
  },
  {
    id: 'tag',
    description: 'data-tag is sent as payload.tag on every event.',
    upstream_ref: [`${TRACKER}:245`, `${TRACKER}:291`],
    page: { tracker: { ...plain, 'data-tag': 'variant-b' } },
    async act(page) {
      await page.evaluate(() => window.umami.track('tagged'));
    },
    wait: 2,
    assert: c => [['all sends tagged', sends(c).length === 2 && sends(c).every(r => r.request_body.payload.tag === 'variant-b')]],
  },
  {
    id: 'auto-track-off',
    description: 'data-auto-track="false" sends no pageview and does not hook clicks or history; umami.track still works.',
    upstream_ref: [`${TRACKER}:246`, `${TRACKER}:652-658`],
    page: { tracker: { ...plain, 'data-auto-track': 'false' }, body: '<button id="b" data-umami-event="ignored">x</button>' },
    async act(page) {
      await page.click('#b');
      await page.evaluate(() => history.pushState({}, '', '/elsewhere'));
      await page.waitForTimeout(600);
      await page.evaluate(() => window.umami.track());
    },
    wait: 1,
    assert: c => [
      ['only the manual pageview', sends(c).length === 1],
      ['manual pageview uses the URL captured at script start', sends(c)[0]?.request_body.payload.url === `${PAGE_ORIGIN}/fixture`],
    ],
  },
  {
    id: 'local-storage-disabled',
    description: 'localStorage "umami.disabled" set disables tracking.',
    upstream_ref: [`${TRACKER}:228-233`, `${TRACKER}:380`],
    page: { tracker: plain },
    init: () => localStorage.setItem('umami.disabled', '1'),
    async act() {},
    expectNone: true,
    assert: c => [['no /api/send request', sends(c).length === 0]],
  },
  {
    id: 'performance',
    description: 'data-performance="true" sends one performance payload after the 10 s timer, with web vitals and duration.',
    upstream_ref: [`${TRACKER}:252`, `${TRACKER}:464-627`],
    page: { tracker: { ...plain, 'data-performance': 'true' }, body: '<h1>Performance</h1><p>Some content.</p>' },
    async act(page) {
      await page.waitForTimeout(11000);
    },
    wait: 2,
    assert: c => {
      const perf = sends(c).find(r => r.request_body.type === 'performance');
      return [
        ['performance payload sent', !!perf],
        ['carries ttfb, fcp and duration', !!perf && ['ttfb', 'fcp', 'duration'].every(k => k in perf.request_body.payload)],
        ['upstream accepted it', perf?.response.status === 200],
      ];
    },
  },
  {
    id: 'bot-user-agent',
    description: 'Default headless Chromium UA ("HeadlessChrome") is flagged by isbot. Upstream answers {beep:"boop"}, the tracker keeps no token, and the recorder never starts because it waits for a token.',
    upstream_ref: ['src/app/api/send/route.ts:142-145', `${TRACKER}:411-415`, `${RECORDER}:636-645`],
    page: { tracker: { 'data-website-id': IDS.websiteRecorder }, recorder: { 'data-website-id': IDS.websiteRecorder }, body: '<p>bot</p>' },
    userAgent: 'default',
    async act(page) {
      await page.waitForTimeout(500);
      await page.evaluate(() => window.umami.track('after-bot'));
      await page.waitForTimeout(6000);
    },
    wait: 1,
    assert: c => [
      ['upstream answered beep boop', sends(c)[0]?.response.body?.beep === 'boop'],
      ['second send has no cache header', sends(c).length === 2 && !('x-umami-cache' in sends(c)[1].request_headers)],
      ['recorder fetched configuration', c.some(r => r.path.endsWith('/recorder'))],
      ['recorder sent nothing', records(c).length === 0],
    ],
  },
  {
    id: 'recorder-replay-and-heatmap',
    description: 'Recorder on a site with replay and heatmaps at sample rate 1 and strict masking: configuration fetch, replay chunks with the tracker cache token, heatmap click and scroll payloads. Masked text and input values never leave the browser.',
    upstream_ref: [`${RECORDER}:18-22`, `${RECORDER}:109-128`, `${RECORDER}:310-322`, `${RECORDER}:432-634`, `${RECORDER}:678-713`],
    page: {
      tracker: { 'data-website-id': IDS.websiteRecorder },
      recorder: { 'data-website-id': IDS.websiteRecorder },
      body: `<main style="height:2400px"><h1>${SECRET_TEXT}</h1><input id="i" type="text"><button id="b">Buy</button></main>`,
    },
    async act(page) {
      await page.waitForTimeout(1500);
      await page.type('#i', SECRET_INPUT);
      await page.click('#b');
      await page.mouse.wheel(0, 1200);
      await page.waitForTimeout(7000);
    },
    wait: 1,
    assert: c => {
      const config = c.find(r => r.path.endsWith('/recorder'));
      const replay = records(c).filter(r => r.request_body?.type === 'record');
      const heatmap = records(c).filter(r => r.request_body?.type === 'heatmap');
      const heatEvents = heatmap.flatMap(r => r.request_body.payload.events);
      return [
        ['configuration enabled with strict masking', config?.response.body?.enabled === true && config.response.body.maskLevel === 'strict'],
        ['replay chunks sent with cache token', replay.length > 0 && replay.every(r => r.request_headers['x-umami-cache'] === '<CACHE_TOKEN>')],
        ['a full snapshot (rrweb type 2) was sent', replay.some(r => r.request_body.payload.events.some(e => e.type === 2))],
        ['every chunk has at most 200 events', replay.every(r => r.request_body.payload.events.length <= 200)],
        ['heatmap click sent', heatEvents.some(e => e.type === 'click')],
        ['heatmap scroll sent', heatEvents.some(e => e.type === 'scroll')],
        ['upstream answered ok:true to every record post', records(c).every(r => r.response.body?.ok === true)],
        ['secret text never transmitted', !c.some(r => r.raw_body.includes(SECRET_TEXT))],
        ['typed input never transmitted', !c.some(r => r.raw_body.includes(SECRET_INPUT))],
      ];
    },
  },
  {
    id: 'recorder-element-exclusion',
    description: 'Recorder on a site whose block selector is .private: the blocked element, its attributes and its text never leave the browser; the rest of the page is still recorded.',
    upstream_ref: [`${RECORDER}:479-495`, 'src/app/api/websites/[websiteId]/recorder/route.ts:32-46'],
    page: {
      tracker: { 'data-website-id': IDS.websiteRecorderPrivate },
      recorder: { 'data-website-id': IDS.websiteRecorderPrivate },
      body: `<main><h1>Checkout</h1><div class="private" data-account="${SECRET_ATTRIBUTE}"><span>${SECRET_TEXT}</span></div><button id="b">Pay</button></main>`,
    },
    async act(page) {
      await page.waitForTimeout(1500);
      await page.click('#b');
      await page.waitForTimeout(3000);
    },
    wait: 1,
    assert: c => {
      const replay = records(c).filter(r => r.request_body?.type === 'record');
      return [
        ['configuration names the block selector', c.find(r => r.path.endsWith('/recorder'))?.response.body?.blockSelector === '.private'],
        ['replay chunks sent', replay.length > 0],
        ['a full snapshot (rrweb type 2) was sent', replay.some(r => r.request_body.payload.events.some(e => e.type === 2))],
        ['blocked attribute never transmitted', !c.some(r => r.raw_body.includes(SECRET_ATTRIBUTE))],
        ['blocked text never transmitted', !c.some(r => r.raw_body.includes(SECRET_TEXT))],
        ['upstream answered ok:true to every record post', records(c).every(r => r.response.body?.ok === true)],
      ];
    },
  },
  {
    id: 'recorder-disabled-site',
    description: 'Recorder on a site without recorder enabled fetches configuration, gets enabled:false, and sends nothing.',
    upstream_ref: [`${RECORDER}:678-702`, 'src/app/api/websites/[websiteId]/recorder/route.ts:28-30'],
    page: { tracker: plain, recorder: plain },
    async act(page) {
      await page.waitForTimeout(3000);
    },
    wait: 1,
    assert: c => [
      ['configuration says disabled', c.find(r => r.path.endsWith('/recorder'))?.response.body?.enabled === false],
      ['no /api/record request', records(c).length === 0],
    ],
  },
  {
    id: 'batch-from-page',
    description: 'A page posts /api/batch cross-origin with two valid items and one invalid one. The answer lists per-item results in order and the first token.',
    upstream_ref: ['src/app/api/batch/route.ts:7-54'],
    page: { tracker: { ...plain, 'data-auto-track': 'false' } },
    async act(page) {
      await page.evaluate(async ([base, website]) => {
        const item = payload => ({ type: 'event', payload: { website, hostname: location.hostname, url: '/batched', ...payload } });
        await fetch(`${base}/api/batch`, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify([item({ name: 'batched-1', data: { n: 1 } }), { type: 'event', payload: { website: 'not-a-uuid' } }, item({ name: 'batched-2' })]),
        });
      }, [BASE_URL, IDS.websitePlain]);
    },
    wait: 1,
    assert: c => {
      const batch = c.find(r => r.path === '/api/batch');
      return [
        ['one batch request', c.filter(r => r.path === '/api/batch').length === 1],
        ['answered 200 with size 3, processed 2, errors 1', batch?.response.status === 200 && batch.response.body?.size === 3 && batch.response.body.processed === 2 && batch.response.body.errors === 1],
        ['details list the invalid item at index 1', batch?.response.body?.details?.[0]?.index === 1],
        ['batch carries a cache token', batch?.response.body?.cache === '<CACHE_TOKEN>'],
      ];
    },
  },
  {
    id: 'link-click',
    description: 'Clicking an anchor to /q/{slug} answers 307 to the link target; the browser follows it.',
    upstream_ref: ['src/app/(collect)/q/[slug]/route.ts:10-63'],
    page: { tracker: { ...plain, 'data-auto-track': 'false' }, body: `<a id="l" href="${BASE_URL}/q/${IDS.linkSlug}">Go</a>` },
    async act(page) {
      await page.route('https://example.com/**', route => route.fulfill({ status: 200, contentType: 'text/html', body: 'landing' }));
      await Promise.all([page.waitForURL(IDS.linkTarget), page.click('#l')]);
    },
    wait: 1,
    assert: c => {
      const hit = c.find(r => r.path === `/q/${IDS.linkSlug}`);
      return [
        ['one link request', c.filter(r => r.path.startsWith('/q/')).length === 1],
        ['307 to the link target', hit?.response.status === 307 && hit.response.headers?.location === IDS.linkTarget],
      ];
    },
  },
  {
    id: 'pixel-image',
    description: 'An <img> pointing at /p/{slug} gets the transparent GIF.',
    upstream_ref: ['src/app/(collect)/p/[slug]/route.ts:10-73'],
    page: { tracker: { ...plain, 'data-auto-track': 'false' }, body: `<img id="p" src="${BASE_URL}/p/${IDS.pixelSlug}" alt="">` },
    async act(page) {
      await page.waitForFunction(() => document.getElementById('p').complete);
    },
    wait: 1,
    assert: c => {
      const hit = c.find(r => r.path === `/p/${IDS.pixelSlug}`);
      return [
        ['one pixel request', c.filter(r => r.path.startsWith('/p/')).length === 1],
        ['200 image/gif', hit?.response.status === 200 && hit.response.headers?.['content-type'] === 'image/gif'],
      ];
    },
  },
];

/* Runner */

const sha = async url => createHash('sha256').update(Buffer.from(await (await fetch(url)).arrayBuffer())).digest('hex');
const scriptHashes = { tracker: await sha(`${BASE_URL}/script.js`), recorder: await sha(`${BASE_URL}/recorder.js`) };

const browser = await chromium.launch();
await mkdir(outDir, { recursive: true });
let failures = 0;

for (const scenario of scenarios) {
  if (only && !scenario.id.startsWith(only)) continue;

  const userAgent = scenario.userAgent === 'default' ? undefined : BROWSER_UA;
  const context = await browser.newContext({
    userAgent,
    locale: 'en-US',
    viewport: { width: 1280, height: 720 },
    screen: { width: 1920, height: 1080 },
  });
  if (scenario.dnt) {
    await context.addInitScript(() => Object.defineProperty(Navigator.prototype, 'doNotTrack', { get: () => '1' }));
  }
  if (scenario.init) {
    await context.addInitScript(`if (location.port === '${PAGE_PORT}') (${scenario.init})();`);
  }

  const page = await context.newPage();
  const pending = [];
  page.on('requestfinished', req => {
    const url = new URL(req.url());
    if (url.origin === BASE_URL && collected(url.pathname) && req.method() !== 'OPTIONS') {
      pending.push(record(req));
    }
  });

  currentPage = { body: '', recorder: null, ...scenario.page };
  await page.goto(`${PAGE_ORIGIN}${scenario.page.path ?? '/fixture'}`, { waitUntil: 'load' });
  await page.waitForTimeout(1000);
  await scenario.act(page);
  await page.waitForTimeout((scenario.expectNone ? 1.5 : scenario.wait) * 1000);
  const captured = await Promise.all(pending);
  await context.close();

  const assertions = scenario.assert(captured).map(([description, passed]) => ({ description, passed: !!passed }));
  const failed = assertions.filter(a => !a.passed);
  failures += failed.length;
  console.log(`${failed.length ? 'FAIL' : 'pass'}  ${scenario.id}  (${captured.length} requests)${failed.map(a => `\n      ${a.description}`).join('')}`);

  const fixture = {
    id: `capture-${scenario.id}`,
    description: scenario.description,
    upstream_ref: scenario.upstream_ref,
    script: {
      tracker_sha256: scriptHashes.tracker,
      ...(scenario.page.recorder && { recorder_sha256: scriptHashes.recorder }),
      served_by: process.env.UMAMI_URL
        ? `server at ${BASE_URL}`
        : 'official Umami 3.4.0 container (tools/collection-harness/compose.yaml)',
    },
    page: {
      url: `${PAGE_ORIGIN}${scenario.page.path ?? '/fixture'}`,
      tracker_attributes: scenario.page.tracker,
      ...(scenario.page.recorder && { recorder_attributes: scenario.page.recorder }),
      ...(scenario.page.head && { head: scenario.page.head }),
      ...(scenario.page.body && { body: scenario.page.body }),
    },
    browser: {
      engine: 'chromium (Playwright 1.63.0, headless)',
      user_agent: userAgent ?? 'Playwright default (contains HeadlessChrome)',
      screen: '1920x1080',
      ...(scenario.dnt && { init: 'navigator.doNotTrack overridden to "1"' }),
      ...(scenario.init && { init: String(scenario.init) }),
    },
    actions: String(scenario.act).split('\n').slice(1, -1).map(l => l.trim()).filter(Boolean),
    captured: captured.map(({ raw_body, ...rest }) => rest),
    assertions,
    validated_against_upstream: !process.env.UMAMI_URL,
    validation_reason: process.env.UMAMI_URL
      ? `Captured from the real scripts talking to ${BASE_URL}; responses are that server's responses.`
      : 'Captured from the real scripts talking to the official Umami 3.4.0 container; responses are upstream responses.',
  };
  await writeFile(path.join(outDir, `${scenario.id}.json`), `${JSON.stringify(fixture, null, 2)}\n`);
}

await browser.close();
server.close();
console.log(failures ? `\n${failures} assertion(s) failed` : '\nall assertions passed');
process.exitCode = failures ? 1 : 0;
