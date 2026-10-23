# Dashboard v2 reference

`index.html` is the visual reference for the [v2 design contract](../../docs/ux-v2/design.md): one rounded canvas inset into a frame, filled controls, hairline panels, the jump field in the sidebar, context and scope chips in the top bar, a headline with a pace projection, a typical-weekday baseline, the finding banner and cards, movers, goal pace, and grouped navigation with attention badges.

Open it directly; it has no dependencies. Values are sample data and nothing is calculated. It shows dark mode only; light mode follows the token table in the design contract.

Where the picture and the contract differ, the contract wins. Colors are hard-coded oklch values from the dark gray ramp and accent in `apps/web/src/assets/css/app.css`; production uses the semantic tokens.

`rejected/first-draft.html` is the first draft, rejected on 2026-10-15 in favor of `index.html` ([design contract, Rejected direction](../../docs/ux-v2/design.md#rejected-direction)). It is kept for the record and is not a reference. Its storage forecast and site-menu content were corrected before it was rejected; the site menu now opens from the site chip in the top bar.
