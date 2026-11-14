export function isTextEditableTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;

  const tagName = target.tagName;
  return (
    tagName === "INPUT" ||
    tagName === "TEXTAREA" ||
    tagName === "SELECT" ||
    target.isContentEditable
  );
}

export const FULL_SCREEN_PANEL_ATTRIBUTE = "data-full-screen-panel";

/**
 * Whether a full-screen panel currently covers the underlying page.
 *
 * Global shortcuts owned by the covered page must not act on hidden controls.
 */
export function hasOpenFullScreenPanel(): boolean {
  return document.querySelector(`[${FULL_SCREEN_PANEL_ATTRIBUTE}]`) !== null;
}
