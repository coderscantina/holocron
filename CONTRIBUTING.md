# Contributing to Holocron

Thank you. Whether you're fixing a typo, squashing a bug, or building a feature, every contribution helps. We're glad you're here.

This guide gets you from zero to merged PR.

Holocron is in beta. Bug reports from real setups are the most useful contribution right now, above all from people moving real Umami data.

---

## Table of contents

- [Code of conduct](#code-of-conduct)
- [Ways to contribute](#ways-to-contribute)
- [Getting started](#getting-started)
- [Commit messages (gitmoji required)](#commit-messages-gitmoji-required)
- [AI-assisted contributions](#ai-assisted-contributions)
- [Pull request process](#pull-request-process)
- [Code style](#code-style)
- [Reporting bugs](#reporting-bugs)
- [Suggesting features](#suggesting-features)

---

## Code of conduct

We expect everyone in this community to be kind, patient, and constructive. Harassment, gatekeeping, and dismissiveness have no place here. The full text lives in [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).

Violations can be reported to [hello@coderscantina.com](mailto:hello@coderscantina.com).

---

## Ways to contribute

You don't have to write code to make a difference:

- **Report a bug.** Even a well-written issue saves someone hours of guesswork
- **Try an import.** Run `holocron import inspect` against a copy of your Umami database and tell us what it says
- **Improve docs.** Spotted something confusing or missing? Fix it
- **Answer questions** in [GitHub Discussions](https://github.com/coderscantina/holocron/discussions)
- **Review pull requests.** Feedback from fresh eyes helps
- **Translate.** The dashboard ships in English and German
- **Write code.** Bug fixes, features, performance work, tests

---

## Getting started

### 1. Fork and clone

```bash
git clone https://github.com/<your-username>/holocron.git
cd holocron
```

### 2. Install tooling

You need Rust 1.99 or newer (with `rustfmt` and `clippy`) and Bun 1.4 or newer. The first build compiles DuckDB from source, which takes a while and a lot of memory.

### 3. Build and run

The Cargo workspace (`apps/server`, `crates/*`) holds the server, and `apps/web` the Vue dashboard. The pinned Umami tracker and recorder live in `sources/`.

Create a data directory once. `init` prints the generated admin password:

```bash
cargo run -p holocron-server --bin holocron -- init --data-dir data --admin-email you@example.com
```

Run the server on `127.0.0.1:8080`, and the dashboard with hot reload on `http://localhost:5173`, which proxies the API to the server:

```bash
cargo run -p holocron-server --bin holocron -- serve --data-dir data
cd apps/web && bun install && bun run dev
```

For an instance with data in it, `scripts/demo-seed.sh` builds, seeds and serves a demo installation with several sites and months of history.

### 4. Create a branch

Use a short, descriptive name:

```bash
git checkout -b fix/funnel-step-order
git checkout -b feat/link-management
```

Prefixes: `feat/`, `fix/`, `docs/`, `refactor/`, `test/`, `chore/`

### 5. Make your changes, then open a PR

Before pushing, run the checks CI runs:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p holocron-protocol --bin export-types -- --check
```

and in `apps/web`:

```bash
bun install --frozen-lockfile
bun run typecheck
bun run lint
bun run test
```

Browser journeys (`bun run e2e`) don't run in CI. They run locally with Playwright against a packaged server whose data directory was seeded within the hour with the `seed_demo`, `seed_traffic` and `seed_commerce` examples, in that order (`cargo run --release -p holocron-server --example <name> -- <data-dir> ...`). Reset the data before every run: journeys write to it. [`apps/web/playwright.config.ts`](apps/web/playwright.config.ts) says how. Run them when you change a screen.

### 6. Read the decision first

Behavior in Holocron is settled in [decision records](docs/decisions/) before it is built: identity, sessions, metric formulas, retention, access, import. If your change touches one of those rules, read the record and say in the PR which one you follow or want to change. A change to a settled rule needs a maintainer's agreement before code.

---

## Commit messages (gitmoji required)

We use **[Gitmoji](https://gitmoji.dev/)** for all commit messages. It keeps the git log easy to scan.

### Format

```
<emoji> <short description in present tense>

[optional body]

[optional footer: closes #issue, co-authors, etc.]
```

### Rules

- Start every commit with exactly one gitmoji
- Use the **imperative, present tense**: "Add feature", not "Added feature"
- Keep the subject line under **72 characters**
- Separate subject from body with a blank line

### Common emojis

| Emoji | Use for                  |
| ----- | ------------------------ |
| ✨    | New feature              |
| 🐛    | Bug fix                  |
| 📝    | Documentation            |
| ♻️    | Refactoring              |
| 🎨    | Code style / structure   |
| ⚡️    | Performance improvement  |
| 🔒    | Security fix             |
| 🧪    | Adding tests             |
| 🏗️    | Architectural change     |
| 🌐    | Internationalization     |
| 🔧    | Config / tooling         |
| 🚀    | Deployment / infra       |
| 🩹    | Minor / non-critical fix |
| 🗑️    | Removing code or files   |
| 🔥    | Removing dead code       |
| 💄    | UI / style changes       |
| 🚸    | UX improvement           |
| 💬    | Text / copy changes      |
| 🏷️    | Types                    |

Full reference: [gitmoji.dev](https://gitmoji.dev/)

### Examples

```
✨ Add a journeys report that starts from any page

Reads session members in time order and stops at the configured depth.

Closes #12
```

```
🐛 Keep the comparison range inside a share's span limit
```

```
📝 Document the CloudFront headers for the realtime globe
```

Commits that do not follow this format will be asked to revise before merging.

---

## AI-assisted contributions

We welcome AI-assisted contributions. Using Copilot, Claude, GPT, Cursor, or similar tools to help write code, tests, or docs is fine. Be transparent about it.

### What we ask

**1. Disclose AI usage in your PR description.**

If any part of your contribution was meaningfully shaped by an AI tool, say so. A single line is enough:

```
> AI assistance: Used Claude to draft the funnel query, then reviewed and adjusted manually.
```

You don't need to disclose autocomplete suggestions or minor one-liners. The rule of thumb: if you'd credit a human pair-programmer, credit the AI.

**2. Understand what you're submitting.**

You are responsible for the code in your PR, not the model that helped generate it. Before opening a PR, make sure you can explain what the code does, why it's correct, and how it was tested. If you can't, keep iterating.

**3. Do not submit AI output without review.**

AI-generated code often looks plausible but contains subtle bugs, security issues, or breaks project conventions. Read the diff and run the tests. Analytics code is easy to get almost right: check the numbers against a case you computed by hand.

**4. No hallucinated dependencies.**

Check that every crate, package, function, or API in AI-generated code exists at the version we use. Phantom imports are a common failure mode.

### Why we have this policy

AI tools are useful and we don't pretend otherwise. Transparency builds trust with maintainers, reviewers, and the people who rely on this codebase. A short disclosure costs you nothing and tells everyone you reviewed what you're shipping.

---

## Pull request process

1. **Open a draft PR early** if you want feedback. You don't need to finish before asking questions
2. **Fill in the PR template.** Describe what changed and why, not just what the diff shows
3. **Link related issues** with `Closes #123` or `Relates to #456`
4. **Keep PRs focused.** One logical change per PR is easier to review and to revert
5. **Respond to review comments.** Maintainers put time into feedback
6. **Squash noise commits** before merge. "fix typo", "oops", and "try again" commits should be folded in

A PR is ready to merge when:

- All CI checks pass
- At least one maintainer has approved
- Conversations are resolved

Releases are cut from `main` by maintainers with `./release.sh`. You don't need to bump versions or edit the changelog.

---

## Code style

### Rust

- `cargo fmt` for formatting and `cargo clippy` with warnings denied
- Shared wire types live in `holocron-protocol`. The generated TypeScript types in `apps/web/src/generated` and `crates/protocol/openapi.json` are checked in CI with `export-types --check`, so regenerate them with `cargo run -p holocron-protocol --bin export-types` when you change the protocol. Never edit generated files by hand
- Keep results exact. Reports count stored events; no sampling, no approximate distinct counts

### TypeScript and Vue

- Run `bun run lint` and `bun run typecheck` before pushing
- Vue components use the Composition API with `<script setup>`
- Tailwind 4 for styling, with the design tokens in `apps/web/src/assets/css/app.css`
- New text goes into both `apps/web/src/i18n/en.json` and `de.json`. A test checks that they match

### General

- Write code for the next person, not just the computer
- Leave things cleaner than you found them
- Avoid over-engineering. Simple and correct beats clever and fragile

---

## Reporting bugs

A good bug report lets someone reproduce the problem without guessing. Please include:

- **Holocron version.** `holocron --version` prints it, for example `26.10.19 (1a2b3c4)`
- **Environment**: OS, architecture, browser if relevant, and what sits in front of Holocron (proxy, CDN)
- **Steps to reproduce**
- **Expected behavior**
- **Actual behavior**
- **Output of `holocron diagnostics`** if the server is involved. It holds no event data or credentials
- **Logs or screenshots** if relevant, with tokens, passwords and database URLs removed

For import problems, add the Umami version and database, and the refusal code or the `differences` from the manifest.

Open a bug report in [GitHub Issues](https://github.com/coderscantina/holocron/issues/new/choose). For security issues, follow the [Security policy](SECURITY.md) instead.

---

## Suggesting features

Feature requests are welcome. Before opening one, check [existing discussions](https://github.com/coderscantina/holocron/discussions) and issues for duplicates.

A good feature request explains:

- The problem you're trying to solve, not just the solution you have in mind
- Who benefits and how
- Whether you'd be willing to help implement it

Start a conversation in [GitHub Discussions](https://github.com/coderscantina/holocron/discussions). Features that gain community traction move up the priority list.

---

## Questions?

If you're unsure about anything, such as scope, approach, or conventions, ask before you invest time writing code. Open a discussion or leave a comment on the relevant issue. We'd rather answer a question upfront than ask you to rewrite a PR.

---

<div align="center">
  <p>Maintained by <a href="https://github.com/badmike">badmike</a> at Coder's Cantina</p>
</div>
