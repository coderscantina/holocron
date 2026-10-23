// Lambda@Edge origin-request handler that copies every Umami tracker hit to Holocron, so both
// collect the same traffic while the sites keep loading Umami. Visitors always get Umami's
// answer; Holocron's copy is best effort. See README.md next to this file.
//
// Lambda@Edge has no environment variables, so set the target here before you deploy.

const HOLOCRON = 'https://analytics.example.com'
const TIMEOUT_MS = 500

// Visitor address and location come from CloudFront; Holocron reads them from a trusted proxy.
const FORWARDED = [
  'content-type',
  'user-agent',
  'origin',
  'referer',
  'accept-language',
  'cloudfront-viewer-address',
  'cloudfront-viewer-country',
  'cloudfront-viewer-country-region',
  'cloudfront-viewer-city',
  'cloudfront-viewer-latitude',
  'cloudfront-viewer-longitude',
]

export const handler = async (event) => {
  const request = event.Records[0].cf.request
  try {
    await mirror(request)
  } catch {
    // Holocron being slow, down or still in setup must never change what the visitor gets. A
    // handler error would: CloudFront answers it with a 5xx instead of Umami's response.
  }
  return request
}

async function mirror(request) {
  if (request.method !== 'POST' || request.uri !== '/api/send' || !request.body?.data || request.body.inputTruncated) {
    return
  }
  const headers = {}
  for (const name of FORWARDED) {
    const value = request.headers[name]?.[0]?.value
    if (value) headers[name] = value
  }
  const body = Buffer.from(request.body.data, request.body.encoding === 'base64' ? 'base64' : 'utf8')
  await fetch(`${HOLOCRON}/api/send`, { method: 'POST', headers, body, signal: AbortSignal.timeout(TIMEOUT_MS) })
}
