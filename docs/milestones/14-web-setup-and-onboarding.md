# Milestone 14: web setup and onboarding

## Outcome

A self-hoster deploys the image on an empty volume and finishes setup in the browser: a fresh install with its first site tracking, a reconciled Umami import, or a restored backup. No shell command is needed. The rules are in [0021](../decisions/0021-web-setup-and-onboarding.md).

## Dependencies and ownership

Requires 11 and 12. Milestone 13 depends on it, and the first public release waits for it. Server owns setup mode, the setup operations and the importer's TLS. Frontend owns the `/setup` route. Protocol changes and locale keys go through their owners.

## Work

1. Design: pick one of the wizard variants and record the rule in [design](../ux-v2/design.md).
2. Setup mode in `serve`: empty, `init_incomplete` and `restore_incomplete` directories; setup token from the log or `HOLOCRON_SETUP_TOKEN`; collection closed; switch to normal serving without a restart.
3. Setup operations in the protocol registry: token check, server check, admin creation with sign-in, first site, import inspect, run, resume and discard, restore and upgrade, and a setup-only progress stream. Regenerate the types.
4. Import inside `serve`: the existing run on the setup phase, progress per step with totals and rate, resume after a restart with the source asked again.
5. Source TLS for PostgreSQL and MySQL, required off private networks, for the CLI and the web alike.
6. The `/setup` route in the Vue app, en and de.
7. Docs: amend 0013 and 0006, CAP-ACCESS-01, the install guide (Docker, compose, Coolify), the Umami migration guide and the runbook.

## Acceptance criteria

- An empty volume reaches a signed-in admin with a first site that has received a pageview, without a shell.
- Both import fixtures import through the wizard with the same manifest digests as the CLI. A server killed during the events step resumes from the wizard to the same digests.
- A backup restores through the wizard, including one from an older release that needs an upgrade.
- Setup operations are refused without the token and once an admin exists. Collection answers 503 throughout setup.
- A plain source connection to a public address is refused; a TLS one is accepted.
- Setup specs pass in e2e against a server on an empty data directory. Screens pass the design's visual acceptance at 1440, 1024, 768 and 390 px, light and dark.
- Rust and web checks from `AGENTS.md` pass.
