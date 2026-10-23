# Security policy

Holocron is a self-hosted web analytics server. One instance takes events from the public internet, holds the browsing history of every tracked site's visitors, session replays, and the accounts and access rules of the teams who read them. We treat collection, site isolation, visitor privacy, authentication, sharing, and release integrity as security-sensitive.

The project is in beta. Releases use calendar versions, e.g. `v26.10.19-1a2b3c4`.

## Supported versions

Security fixes are guaranteed for the latest release. Older releases do not receive backported patches.

| Release | Status        |
| ------- | ------------- |
| Latest  | Supported     |
| Older   | Not supported |

## Reporting a vulnerability

**Do not open a public GitHub issue for an unpatched vulnerability.**

Report suspected security issues through one of these channels, in order of preference:

1. **GitHub Security Advisories**, using private reporting:
   [Submit a security advisory](../../security/advisories/new)

2. **Email** to `security@coderscantina.com`
   Subject line: `Holocron security report`

Please include as much of the following as possible:

- Holocron version (`holocron --version`, release tag or commit SHA)
- Environment: Docker or source build, OS, and the proxy or CDN in front of Holocron
- Affected component (collection, tracker or recorder routes, dashboard API, sign-in, sharing, replays, import, operator commands, etc.)
- Reproduction steps or proof of concept
- Impact summary: what data or access is at risk and for whom
- Your estimated CVSS 3.1 score (optional, we may adjust)

You don't need to request a CVE. We handle that as part of the advisory process.

## Response expectations

- Acknowledgement within **72 hours**.
- Status update within **14 calendar days** if a fix is not yet available.
- We coordinate public disclosure after a fix or mitigation is ready.
- Credit is given to reporters in the advisory and release notes, with your consent.

## Disclosure policy

Once a fix is prepared:

1. A patch release is issued.
2. Release notes say that a security issue was addressed.
3. Detailed disclosure is delayed by 2 to 8 weeks depending on severity, so operators can upgrade.
4. The GitHub Security Advisory and CVE (if applicable) are published after the waiting period.

Please contact us before any public disclosure.

## What we consider a security issue

- **Collection abuse.** Writing events, replays or heatmap data for a site, link or pixel you don't control: cache tokens accepted for another site, collection keys accepted for another site, or unknown source IDs stored
- **Collection spoofing.** Choosing your own visitor address, pseudonym or location without a collection key or from an untrusted proxy, or getting around `HOLOCRON_COLLECT_IGNORE_IPS`
- **Resource exhaustion through collection.** Requests that get past the body caps, the admission budget, the disk reserve or the replay cap, or that make the server acknowledge events it did not store
- **Site and workspace isolation.** Reading or changing another workspace's sites, reports, replays or exports without a role there, or an install admin reading analytics without membership
- **Authorization flaws.** Privilege escalation between roles, missing capability checks, sensitive commands that skip reauthentication, or an API key acting beyond its operations and sites
- **Share links.** A share revealing measures, fields, properties, dates or replays outside its limits, working after expiry or revocation, or granting more than its creator holds
- **Replay privacy.** Unmasked text or input values reaching storage from the supported recorder, page exclusions not deleting what they should, or captured content running script or loading resources during playback
- **Visitor privacy.** IP addresses, user agents or identify IDs written to storage, logs or diagnostics, daily keys surviving their deletion time, or erasure leaving a profile's data behind
- **Authentication and sessions.** Sign-in, two-factor or recovery code bypass, session fixation or theft, cross-site request forgery on dashboard commands
- **Import.** The importer writing to a source database, accepting credentials that can write, leaking source credentials, or unsafe handling of the files and paths it reads
- **Operator commands and backups.** Backup, restore or upgrade exposing `secrets.key` or account data beyond the documented file modes, or path handling that writes outside the given directories
- **Injection.** SQL, command or template injection, including through event properties, filters and report names
- **Release integrity.** Tampered images or artifacts, or supply chain issues in the build

## What usually does not qualify

- Cosmetic UI defects
- Performance issues without a plausible security impact
- `/health/ready` naming problems such as `disk_full`. It is public by design
- A visitor's own browser sending data before a server-side page exclusion drops it. This is documented
- Consent signals sent by a site's own page. Holocron accepts them by design; the site operator is responsible for collecting consent
- Misconfiguration by an operator that matches documented behavior, such as trusting a proxy range that includes untrusted clients
- Dependency version bumps, unless they introduce a new vulnerability surface in Holocron
- Unsupported environments

If you are unsure whether something qualifies, report it privately first. We would rather triage a non-issue than miss a real one.

## Other platforms

We do not monitor third-party vulnerability disclosure platforms. All reports must come through GitHub Security Advisories or `security@coderscantina.com`. Direct messages to maintainers on other channels are not a supported reporting path.
