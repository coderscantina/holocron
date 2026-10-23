// Replays fixtures/collection/*.json against a running upstream Umami and compares
// status, selected headers, and body shape with each fixture's expected_upstream.
//
// Usage: node tools/collection-harness/replay.mjs [--pass default|collect-alias] [--only <id-prefix>] [--markdown]
import { createHash } from 'node:crypto';
import { readdir, readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { BASE_URL, BROWSER_UA } from './lib.mjs';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const fixtureDir = path.join(root, 'fixtures/collection');

const args = process.argv.slice(2);
const option = name => {
  const i = args.indexOf(name);
  return i === -1 ? undefined : args[i + 1];
};
const pass = option('--pass') ?? 'default';
const only = option('--only');
const markdown = args.includes('--markdown');

const tokens = new Map();
/** Unique per runner invocation so eventId fixtures do not collide across runs. */
const runId = Date.now().toString(36);

/** Obtains a real cache token from upstream by sending one pageview for the website. */
async function cacheToken(websiteId) {
  if (!tokens.has(websiteId)) {
    const res = await fetch(`${BASE_URL}/api/send`, {
      method: 'POST',
      headers: { 'content-type': 'application/json', 'user-agent': BROWSER_UA },
      body: JSON.stringify({
        type: 'event',
        payload: { website: websiteId, url: '/cache-bootstrap', hostname: 'localhost' },
      }),
    });
    const { cache } = await res.json();
    if (!cache) throw new Error(`no cache token for ${websiteId}`);
    tokens.set(websiteId, cache);
  }
  return tokens.get(websiteId);
}

async function resolve(value) {
  if (Array.isArray(value)) return Promise.all(value.map(resolve));
  if (value && typeof value === 'object') {
    if ('$repeat' in value) {
      const item = await resolve(value.item);
      return Array.from({ length: value.$repeat }, () => structuredClone(item));
    }
    const entries = await Promise.all(
      Object.entries(value).map(async ([k, v]) => [k, await resolve(v)]),
    );
    return Object.fromEntries(entries);
  }
  if (typeof value !== 'string') return value;

  const now = value.match(/^\{\{NOW_S([+-]\d+)?\}\}$/);
  if (now) return Math.floor(Date.now() / 1000) + Number(now[1] ?? 0);

  const cache = value.match(/^\{\{CACHE:([0-9a-f-]{36})\}\}$/);
  if (cache) return cacheToken(cache[1]);

  return value
    .replace(/\{\{FILL:(\d+)\}\}/g, (_, n) => 'x'.repeat(Number(n)))
    .replaceAll('{{RUN_ID}}', runId);
}

const TYPE_TOKENS = {
  '<any>': () => true,
  '<string>': v => typeof v === 'string',
  '<number>': v => typeof v === 'number',
  '<boolean>': v => typeof v === 'boolean',
  '<uuid>': v => typeof v === 'string' && /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.test(v),
  '<jwt>': v => typeof v === 'string' && v.split('.').length === 3,
  '<null>': v => v === null,
  '<array>': v => Array.isArray(v),
  '<object>': v => !!v && typeof v === 'object' && !Array.isArray(v),
};

/** Returns a list of mismatch descriptions; empty means the actual value matches. */
function match(expected, actual, exact, at = 'body') {
  if (typeof expected === 'string' && expected in TYPE_TOKENS) {
    return TYPE_TOKENS[expected](actual) ? [] : [`${at}: expected ${expected}, got ${JSON.stringify(actual)}`];
  }
  if (Array.isArray(expected)) {
    if (!Array.isArray(actual) || actual.length !== expected.length) {
      return [`${at}: expected array of ${expected.length}, got ${JSON.stringify(actual)?.slice(0, 120)}`];
    }
    return expected.flatMap((e, i) => match(e, actual[i], exact, `${at}[${i}]`));
  }
  if (expected && typeof expected === 'object') {
    if (!actual || typeof actual !== 'object' || Array.isArray(actual)) {
      return [`${at}: expected object, got ${JSON.stringify(actual)?.slice(0, 120)}`];
    }
    const problems = Object.entries(expected).flatMap(([k, e]) =>
      e === '<absent>'
        ? k in actual ? [`${at}.${k}: expected absent`] : []
        : k in actual ? match(e, actual[k], exact, `${at}.${k}`) : [`${at}.${k}: missing`],
    );
    if (exact) {
      for (const k of Object.keys(actual)) if (!(k in expected)) problems.push(`${at}.${k}: unexpected key`);
    }
    return problems;
  }
  return Object.is(expected, actual) ? [] : [`${at}: expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)?.slice(0, 120)}`];
}

async function send(request) {
  const headers = { 'user-agent': BROWSER_UA };
  let body;

  if (request.body_raw !== undefined) {
    body = await resolve(request.body_raw);
  } else if (request.body !== undefined) {
    body = JSON.stringify(await resolve(request.body));
    headers['content-type'] = 'application/json';
  }
  for (const [k, v] of Object.entries(await resolve(request.headers ?? {}))) {
    if (v === null) delete headers[k.toLowerCase()];
    else headers[k.toLowerCase()] = v;
  }

  return fetch(`${BASE_URL}${request.path}`, {
    method: request.method,
    headers,
    body,
    redirect: 'manual',
    signal: AbortSignal.timeout(120_000),
  });
}

async function run(fixture) {
  const { request, expected_upstream: expected } = fixture;
  for (const step of fixture.setup ?? []) {
    const res = await send(step);
    await res.arrayBuffer();
    if (!res.ok) throw new Error(`setup ${step.method} ${step.path} returned ${res.status}`);
  }
  const res = await send(request);
  const buffer = Buffer.from(await res.arrayBuffer());
  const problems = [];

  if (res.status !== expected.status) problems.push(`status: expected ${expected.status}, got ${res.status}`);

  for (const [name, want] of Object.entries(expected.headers ?? {})) {
    const got = res.headers.get(name);
    if (want === '<absent>' ? got !== null : want === '<present>' ? got === null : got !== want) {
      problems.push(`header ${name}: expected ${want}, got ${got}`);
    }
  }

  if (expected.body_sha256) {
    const sha = createHash('sha256').update(buffer).digest('hex');
    if (sha !== expected.body_sha256) problems.push(`body sha256: got ${sha}`);
  }
  for (const needle of expected.body_contains ?? []) {
    if (!buffer.includes(needle)) problems.push(`body does not contain ${JSON.stringify(needle)}`);
  }
  if (expected.body_bytes !== undefined && buffer.length !== expected.body_bytes) {
    problems.push(`body bytes: expected ${expected.body_bytes}, got ${buffer.length}`);
  }
  if (expected.body !== undefined) {
    let parsed;
    try {
      parsed = JSON.parse(buffer.toString('utf8'));
    } catch {
      parsed = buffer.toString('utf8');
    }
    problems.push(...match(expected.body, parsed, expected.body_match === 'exact'));
  }

  return { id: fixture.id, surface: fixture.surface, status: res.status, problems };
}

const files = (await readdir(fixtureDir)).filter(f => f.endsWith('.json')).sort();
const fixtures = [];
for (const file of files) {
  const fixture = JSON.parse(await readFile(path.join(fixtureDir, file), 'utf8'));
  if (only && !fixture.id.startsWith(only)) continue;
  if ((fixture.requires_pass ?? 'default') !== pass) continue;
  if (!fixture.validated_against_upstream) continue;
  fixtures.push(fixture);
}

const results = [];
for (const fixture of fixtures) {
  try {
    results.push(await run(fixture));
  } catch (e) {
    results.push({ id: fixture.id, surface: fixture.surface, status: null, problems: [`error: ${e.message}`] });
  }
}

for (const r of results) {
  const verdict = r.problems.length ? 'FAIL' : 'pass';
  if (markdown) console.log(`| ${r.id} | ${r.surface} | ${r.status ?? '-'} | ${verdict} | ${r.problems.join('; ')} |`);
  else console.log(`${verdict}  ${r.id}  (${r.status})${r.problems.map(p => `\n      ${p}`).join('')}`);
}
const failed = results.filter(r => r.problems.length).length;
console.log(`\n${results.length - failed}/${results.length} passed (pass=${pass}, ${BASE_URL})`);
process.exitCode = failed ? 1 : 0;
