# 0020: Managing links and pixels from the dashboard

Status: Adopted by Mike on 2026-10-21. It amends the link and pixel parts of [0003](0003-metric-formulas.md) Links and pixels and [0004](0004-exploration-state-and-limits.md) Sources, and the planned capabilities of CAP-TRAFFIC-12 to 15 in the [capability matrix](../contracts/capability-matrix.md).

## Problem

Holocron answers `/q/<slug>` and `/p/<slug>` like Umami, but links and pixels could only come from an Umami import. Nobody could add one, fix a target URL or read a link's hits. Umami users had all of that in its Links and Pixels screens.

## Choice

### Scope and capabilities

- A link or pixel belongs to one workspace, as the `links` and `pixels` tables already said and as Umami scoped them to a user or a team. Holocron's personal workspaces stand in for Umami's user-owned ones.
- A role has the same capabilities on the workspace's links and pixels as on one of its sites:
  - `site.read` lists them and reads their reports: every role.
  - `site.configure` renames them, changes a link's URL and changes a slug: members and above.
  - `site.delete` deletes them: admins and owners, after a password check within the last 10 minutes.
  - Creating one needs `workspace.write`, like adding a site: members and above.
- API keys reach them only when they hold every site, like boards ([0012](0012-access-implementation.md)). Share viewers never do. Deleting can never be granted to a key.
- The capability matrix planned `workspace.write` for deletion. Deletion erases the hits, so it follows site deletion instead ([0007](0007-access-and-account-security.md) Member and site deletion).

### Slugs

- A slug is 1 to 100 ASCII letters, digits, `-` or `_`, trimmed. It stays one path segment that needs no encoding. Imported slugs keep whatever Umami allowed.
- Slugs are unique among links and, separately, among pixels. `/q/` and `/p/` are separate routes, so a link and a pixel may share one, as in Umami (`link_slug_key` and `pixel_slug_key` are separate unique indexes).
- Without a slug the server picks 8 random characters from `a` to `z` and `0` to `9`.
- A deleted imported link or pixel keeps its slug reserved: the table's unique constraint covers it. A slug in use is refused with `invalid_arguments`, `slug: invalid_value`.

### Changing a slug

The new slug answers at once and the old one stops answering and becomes free, as in Umami. The edit dialog says so before saving. Holocron keeps no alias, so an old printed address breaks. Keeping old slugs as redirects would need an alias table, rules for when an alias expires and what happens when someone else wants it. That is worth adding only when someone asks for it.

### Link targets

- A target is an absolute `http` or `https` URL with a host and no credentials, at most 2048 characters. It is stored in its normalized form, which is ASCII and always a valid `Location` header.
- `javascript:`, `data:` and other schemes are refused. Credentials are refused because `https://bank.example@evil.example/` reads like the bank.
- `/q/<slug>` is not an open redirect: the target comes from a person who may change the link, never from the request that follows it.

### Deleting

Deleting removes the row, which frees the slug, and queues the erasure job that removes a deleted site's data. It works on any analytical source. A soft delete would keep events that nothing could show and that retention would still have to find.

### Reports

- `links.explore` and `pixels.explore` take the same arguments as `analytics.explore`. The state's `site` holds the link or pixel ID, as 0004 Sources says. The operation names the source instead of a `source` field in the state, so the state, its URL form and the site scope code stay as they are.
- Hits count as `events`; `visitors` and `visitor_days` count daily pseudonyms, which use UTC days for these sources ([0001](0001-identity-and-consent.md)). 0003 named the count `hits`. A new measure would need its own validation, labels and catalog entries, for a number `events` already gives. Other measures and reports are refused.
- These reports read events only, never the site rollups.
- The dashboard opens them on the last 30 days in UTC, with referrers, countries, browsers and devices. It reuses the overview's measure cards, chart and breakdowns.
- `lost_hits` is a total since creation, not per range. The report shows it when it is above zero. 0003 asked for a count per range, which needs a dated counter.

### Retention

Links and pixels created in the dashboard follow the installation default, as [0016](0016-configurable-retention.md) says, and there is still no per-link setting. Nobody asked for one, and the default already covers the case.

## Rejected alternatives

- **One slug namespace for links and pixels.** Routing does not need it and Umami does not have it, so imported data could break the rule.
- **A `source` field in the exploration state.** It changes the state every URL, saved view and board card carries, and every site scope check would have to learn about links.
- **Members may delete, as with boards.** A board holds no collected data. A link's hits do.
- **Keep the old slug answering after a rename.** See Changing a slug.

## Consequences

- The overview's breakdown panel gained an `explorable` switch, since links and pixels have no Explore page.
- A deleted imported link still blocks its slug. Freeing it would need a migration to a partial unique index.
- There are no `links.get` and `pixels.get` operations. The report page reads the workspace list, which is small.

## Affected contracts

0003 Links and pixels, 0004 Sources, CAP-TRAFFIC-12 to 15, the access matrix (`fixtures/access/cases.json`, AX-G1182 to AX-G1301 and AX-T178 to AX-T180, resources `link_a1` and `pixel_a1`), protocol `links.*` and `pixels.*`, `LinkId`, `PixelId`, the limits `SLUG_MAX_CHARS`, `SLUG_GENERATED_CHARS` and `LINK_URL_MAX_CHARS`, and the guide's [tracking](../guide/tracking.md#links-and-pixels), [Umami migration](../guide/umami-migration.md) and [teams and access](../guide/teams-and-access.md) pages.
