// Shared constants for the collection harness. Fixture files reference these IDs literally.
export const BASE_URL = process.env.UMAMI_URL || 'http://127.0.0.1:33400';

export const IDS = {
  websitePlain: '5e1a0000-0000-4000-8000-000000000001',
  websiteRecorder: '5e1a0000-0000-4000-8000-000000000002',
  websiteHeatmapOnly: '5e1a0000-0000-4000-8000-000000000003',
  websiteReplayOnly: '5e1a0000-0000-4000-8000-000000000004',
  websiteRecorderPrivate: '5e1a0000-0000-4000-8000-000000000005',
  websiteUnknown: '5e1a0000-0000-4000-8000-0000000000ff',
  link: '5e1a0000-0000-4000-8000-0000000000a1',
  linkSlug: 'holocron-link',
  linkTarget: 'https://example.com/landing?from=holocron',
  pixel: '5e1a0000-0000-4000-8000-0000000000b1',
  pixelSlug: 'holocron-pixel',
};

/** A desktop Chrome user agent that isbot does not flag. Headless Chromium's default UA is flagged. */
export const BROWSER_UA =
  'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36';
