# Milestone 03: Kit shell and generated API

## Outcome

A real Vue application runs inside the Rust deployment, using extracted Kit components and a generated command/query interface. The shell is ready for live analytics, not a second HTML prototype.

## Dependencies and ownership

Requires 01. Frontend owns Kit extraction, routing, tokens, and screens. Protocol owns Rust wire types, registry, export tooling, and typed browser resources. Integration owns startup and static hosting. Read [Kit implementation](../kit-implementation.md) and the Folio source links in [the briefing](../technical-briefing.md) and [architecture patterns](../contracts/architecture-patterns.md).

## Work

1. Scaffold the proposed Rust server/protocol and Vue application paths. Pin versions from inspected source/locks. Provide focused check commands.
2. Adapt Kit AppLayout, AppHeader, AppSidebar, AppUserMenu, navigation, command palette, theme, locale, and accessible page states. Replace Laravel-specific auth/resources/realtime dependencies with narrow Rust adapters.
3. Implement the Clerk-style workspace / website / environment header controls. The account dropdown sits at the right edge. Account identity and account links do not appear in the sidebar. Environment display remains contextual and adds no multi-server architecture.
4. Generate operation maps and wire types from the actual Rust dispatcher. Define command idempotency, typed errors, field validation, expected revisions, cancellation, and query argument limits.
5. Implement bootstrap/login, site selection, and opaque authorized SiteScope sufficient to protect the first real data slices. Add origin protection for authenticated mutations while leaving browser collection CORS separate.
6. Serve the built SPA with runtime configuration. Unknown API routes return API errors, not HTML fallback. Use appropriate caching for hashed assets and revalidated HTML/config.
7. Connect at least one real scoped site read and update through generated Vue resources. Fixtures may render overview temporarily, with explicit labels.

## Acceptance criteria

- The packaged Rust server serves the Vue shell without a production Node process.
- Generated schema regeneration produces no unexplained drift. Dispatch, types, errors, and API documentation agree.
- Site access is enforced server-side, including a guessed inaccessible ID.
- Header/menu behavior, light/dark modes, phone navigation, keyboard focus, and translated copy meet the Kit contract.
- Component reuse is recorded in an extraction map with replaced dependencies.
- A runtime-config change takes effect without rebuilding Vue.

## Evidence and handoff

Run focused Rust HTTP tests, Vue type/lint checks, generated-contract checks, and a real browser workflow. Save shell screenshots and API fixtures. Hand the usable shell and protected resources to 04 through 10. Full account features close in 10.
