## Summary

<!-- What does this PR change, and why? Describe the intent, not just the diff. -->

## Linked issue

<!-- Closes #123 / Relates to #456, or "none" for small standalone fixes. -->

## Decision records

<!-- Which docs/decisions/ records does this follow or change? "none" if it touches no settled rule. -->

## How was this tested?

<!-- Manual steps, test cases added, edge cases considered. For report changes, a case you checked by hand. -->

## AI assistance

<!-- If any part of this contribution was meaningfully shaped by an AI tool, say so.
     The spirit of the rule: if you'd credit a human pair-programmer, credit the AI.
     Example:
     > AI assistance: Used Claude to draft the funnel query, then reviewed and adjusted manually.
     Autocomplete suggestions and minor one-liners don't need disclosure. -->

## Checklist

- [ ] Commits follow the [Gitmoji format](CONTRIBUTING.md#commit-messages-gitmoji-required)
- [ ] `cargo fmt --all --check` and `cargo clippy --workspace --all-targets -- -D warnings` pass
- [ ] Generated types are current (`cargo run -p holocron-protocol --bin export-types -- --check`)
- [ ] `bun run lint`, `bun run typecheck` and `bun run test` pass for changes in `apps/web`
- [ ] Tests pass locally (and new behavior is covered where it makes sense)
- [ ] AI usage is disclosed above (or not applicable)
- [ ] I understand and can explain the code I'm submitting
