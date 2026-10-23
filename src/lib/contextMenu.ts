const SELECTABLE_SELECTOR =
  'input, textarea, [contenteditable="true"], [contenteditable=""], pre, code, .selectable';

/**
 * Suppress the WebView's default context menu (macOS: Look Up / Share /
 * Services) on app chrome, matching the global `user-select: none` policy in
 * index.css. Editable fields and selectable content keep the native menu so
 * Copy still works there.
 */
export function initializeContextMenuGuard(): void {
  document.addEventListener("contextmenu", (event) => {
    if (
      event.target instanceof Element &&
      event.target.closest(SELECTABLE_SELECTOR)
    ) {
      return;
    }
    event.preventDefault();
  });
}
