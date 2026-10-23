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
    // A live selection means the user is working with copyable text (e.g.
    // Tailwind `select-text` areas): keep the native menu for Copy.
    if (window.getSelection()?.toString()) {
      return;
    }
    if (
      event.target instanceof Element &&
      event.target.closest(SELECTABLE_SELECTOR)
    ) {
      return;
    }
    event.preventDefault();
  });
}
