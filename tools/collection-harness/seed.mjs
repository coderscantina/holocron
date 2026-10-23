// Seeds the upstream Umami instance with the fixed sources used by fixtures/collection.
// Uses Umami's own API with the default admin account created by the 01_init migration.
// Idempotent: existing sources are updated, not duplicated.
import { BASE_URL, IDS } from './lib.mjs';

async function api(token, method, path, body) {
  const res = await fetch(`${BASE_URL}${path}`, {
    method,
    headers: {
      'content-type': 'application/json',
      ...(token && { authorization: `Bearer ${token}` }),
    },
    body: body && JSON.stringify(body),
  });
  const data = await res.json().catch(() => null);
  return { status: res.status, data };
}

async function ensure(token, getPath, createPath, body) {
  const existing = await api(token, 'GET', getPath);
  if (existing.status === 200 && existing.data?.id) return existing.data;
  const created = await api(token, 'POST', createPath, body);
  if (created.status !== 200) {
    throw new Error(`POST ${createPath} failed: ${created.status} ${JSON.stringify(created.data)}`);
  }
  return created.data;
}

const login = await api(null, 'POST', '/api/auth/login', { username: 'admin', password: 'umami' });
if (login.status !== 200) throw new Error(`login failed: ${login.status}`);
const { token } = login.data;

const websites = [
  { id: IDS.websitePlain, name: 'holocron plain', replayConfig: null },
  {
    id: IDS.websiteRecorder,
    name: 'holocron recorder',
    replayConfig: {
      replayEnabled: true,
      heatmapEnabled: true,
      sampleRate: 1,
      heatmapSampleRate: 1,
      maskLevel: 'strict',
    },
  },
  {
    id: IDS.websiteHeatmapOnly,
    name: 'holocron heatmap only',
    replayConfig: { replayEnabled: false, heatmapEnabled: true, heatmapSampleRate: 1 },
  },
  {
    id: IDS.websiteReplayOnly,
    name: 'holocron replay only',
    replayConfig: { replayEnabled: true, heatmapEnabled: false, sampleRate: 1 },
  },
  {
    id: IDS.websiteRecorderPrivate,
    name: 'holocron recorder private',
    replayConfig: { replayEnabled: true, heatmapEnabled: false, sampleRate: 1, maskLevel: 'strict', blockSelector: '.private' },
  },
];

for (const { id, name, replayConfig } of websites) {
  await ensure(token, `/api/websites/${id}`, '/api/websites', { id, name, domain: 'localhost' });
  if (replayConfig) {
    const res = await api(token, 'POST', `/api/websites/${id}`, { replayConfig });
    if (res.status !== 200) throw new Error(`update ${id} failed: ${res.status}`);
  }
}

await ensure(token, `/api/links/${IDS.link}`, '/api/links', {
  id: IDS.link,
  name: 'holocron link',
  url: IDS.linkTarget,
  slug: IDS.linkSlug,
});

await ensure(token, `/api/pixels/${IDS.pixel}`, '/api/pixels', {
  id: IDS.pixel,
  name: 'holocron pixel',
  slug: IDS.pixelSlug,
});

console.log(`seeded ${BASE_URL}: 5 websites, 1 link, 1 pixel`);
