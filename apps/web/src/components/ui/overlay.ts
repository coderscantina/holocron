/**
 * The one scrim. Every modal surface (dialog, alert dialog, sheet) dims the
 * page with the same tone, so opening one from another does not step the
 * background through two different greys. `bg-overlay` is the scrim rung from
 * `app.css`; the blur is what separates a dialog from the page behind it.
 */
export const overlayClass =
  'fixed inset-0 z-50 bg-overlay backdrop-blur-xs data-[state=open]:animate-in data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0'
