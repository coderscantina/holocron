# Status

Updated 2026-10-23. Read by agents at the start of a thread; rewrite it before handing back (`project-status` skill). Deployment details, accounts and local processes live in `.demo/NOTES.md`, which git ignores; keep them out of this file.

## Branches

The branch of record is `main`, published to https://github.com/coderscantina/holocron as one initial commit. Release `v26.10.23-4d4c828` is out (2026-10-23), with the image `ghcr.io/coderscantina/holocron:26.10.23` for amd64 and arm64. Milestone 14's fresh and import paths are in it.

## Waiting on Mike

- Set the `holocron` package to public on GitHub, so it pulls without credentials, and upload `.github/social-preview.png` as the social preview.
- Decision 0008 is still Proposed, pending the storage gate.

Settled on 2026-10-23: the fresh and import paths ship now, restore through the wizard, 0006 and CAP-ACCESS-01 follow in a later release; an unencrypted or writable import source is a warning, not a refusal; the install-wide first-hit marker, host names judged as written, `sslmode=require` as TLS and setup sign-in for a Holocron admin without 2FA stay as built; "Continue without import" is offered on every import step.

## Next

1. Mike's production move from Umami (`.demo/NOTES.md`), starting with ops01 in setup mode.
2. The rest of milestone 14: restore and upgrade through the wizard, 0006 and CAP-ACCESS-01, the setup screens' visual acceptance at four widths.
3. Milestone 13, the integrated release gate ([milestones/README.md](milestones/README.md)), and the 0008 storage gate, about a month after the production move on a rented 2 vCPU / 4 GiB host.
