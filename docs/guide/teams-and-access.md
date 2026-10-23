# Teams and access

Who can see which site, how people sign in, and how scripts and outsiders get in. The rules come from [decision 0007](../decisions/0007-access-and-account-security.md) and [decision 0012](../decisions/0012-access-implementation.md).

## Workspaces and sites

Every site belongs to one workspace:

- **Teams** are shared workspaces. Anyone signed in can create one and becomes its owner. `holocron init` creates a team called Default with the first admin as owner.
- **Personal workspaces** belong to one person. One is created when someone moves a site there, and the Umami import creates them for personal Umami sites.

Members and above add sites to a workspace with a name, a domain and a timezone. A site's timezone decides where its days start, for reports and for the daily visitor key. Admins and owners can move a site to another workspace. Its shares and API key grants end with the move.

## Roles

People get a role in a team. The role applies to every site in it.

| Can | Viewer | Member | Admin | Owner |
| --- | --- | --- | --- | --- |
| Read reports, export, watch replays | yes | yes | yes | yes |
| Add sites, edit boards | | yes | yes | yes |
| Add and change links and pixels | | yes | yes | yes |
| Save segments, goals, annotations and shared explorations | | yes | yes | yes |
| Change site settings: tracking, recording, identity, sessions | | yes | yes | yes |
| Create share links | | yes | yes | yes |
| Change a site's retention, delete a site or its data, move it to another workspace | | | yes | yes |
| Delete links and pixels | | | yes | yes |
| Invite people, change roles, remove members, require 2FA for the team | | | yes | yes |

Nobody can grant a role at or above their own, with one exception: owners can make other owners, and can demote them. The last owner can't leave or be demoted, so no team is left without one.

Members keep everything except deletion. In Umami, members can delete sites. Holocron moved that to admins and owners on purpose, and does the same for links and pixels, since deleting erases collected data.

Links and pixels belong to the workspace, and every role has the same rights on them as on the workspace's sites. See [Links and pixels](tracking.md#links-and-pixels).

## Install admins

The install admin role is separate from team roles. Install admins:

- create accounts, edit them, reset a person's password or second factor, and delete accounts
- set the security policy, such as two-factor sign-in for everyone
- set the default data retention for the installation
- see Server status: health, lag, disk and replay budgets

Install admins don't read a site's analytics, replays or exports unless they are a member of the site's workspace. Running the server is not the same as being allowed to read every customer's visitors. An install admin who needs to look adds themselves to the team, which shows in the member list.

Deleting an account is refused while that person is the only owner of a workspace or has personal sites. Move or delete those first. The account's boards, segments and shares stay, shown as "Deleted user". Its private explorations and API keys go.

## Accounts and invitations

Holocron sends no email. Accounts are created by install admins only, with a generated password that is shown once. There is no self sign-up.

To bring someone into a team, an owner or admin creates an invitation for one role, with a label and a lifetime of 1 to 30 days (7 by default). The invitation is a single-use link, `/teams/join?code=<code>`, shown once. Pass it on however you like. The person signs in and opens it.

## Sign-in and sessions

- Sessions last 30 days at most and end after 7 days without use.
- Changing your password signs you out everywhere else. Account, then Security, can end your other sessions too.
- Sensitive actions ask for your password again if you haven't entered it in the last 10 minutes: changing your password or second factor, creating an API key, raising someone to Admin or Owner, deleting, resetting or moving a site, deleting a link or pixel, deleting a team or an account, resetting someone's sign-in, and changing the security policy.

## Two-factor sign-in

Set it up under Account, then Security. Holocron uses time-based codes (TOTP: 6 digits, 30 seconds), which work with common authenticator apps, and gives you 10 recovery codes. Each recovery code works once.

Two-factor can be required for the whole installation, for a team, or for one person. Someone who must use it but hasn't set it up can sign in, but can do nothing except set it up. A requirement switched on applies at the person's next request.

Without email, account recovery is a recovery code, or an install admin who resets the second factor.

The secrets of authenticator apps are encrypted with `secrets.key` in the data directory. See [Secrets key](configuration.md#secrets-key).

## API keys

Create keys under Account, then API keys. A key:

- acts as you, with your current role, so it can never do more than you can
- is limited to the operations you pick and to all your sites or a list of them
- is sent as `Authorization: Bearer <key>`
- is shown once and stored only as a hash

The sensitive actions listed under [Sign-in and sessions](#sign-in-and-sessions) can never be granted to a key. A key limited to a list of sites can't reach workspace-wide things like boards, links, pixels and members.

The API is the one the dashboard uses: named queries at `GET /api/queries/<name>` and named commands at `POST /api/commands/<name>`. [`crates/protocol/openapi.json`](../../crates/protocol/openapi.json) describes every operation, its arguments and its result.

## Collection keys

Collection keys are for your own servers sending events. They are per site, up to 20 active at once, and created by anyone who can change the site's settings. See [Server-side events](tracking.md#server-side-events).

## Share links

Share links show a site's analytics to people without an account. Create them in the site's settings under Sharing. A share fixes:

- the measures and fields viewers may see, and which custom properties
- the longest span viewers may read, 1 to 366 days. A comparison counts toward it, so a 30-day share allows 15 days with a previous-period comparison
- an optional filter every view keeps, and whether viewers may add filters of their own
- whether viewers may export, and whether they may watch replays inside the share's range and filter
- when it expires, at most 366 days ahead

Boards can be shared too. A share keeps a copy of what was shared, so it survives edits to the saved view and the deletion of its author. Revoking it ends access at the viewer's next read.

The link is `/share/<token>`. The dashboard sends the token in a header, never in an API URL, so it doesn't end up in access logs. A share never grants more than its creator has.
