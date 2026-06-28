// Runtime detection of execution environment
// This replaces VITE_CC_SWITCH_MODE with automatic detection

/**
 * Check if running inside Tauri desktop app
 * Tauri injects `__TAURI__` into the window object
 */
export function isTauri(): boolean {
  return (
    typeof window !== "undefined" &&
    // @ts-ignore - Tauri global
    !!(window.__TAURI__ || window.__TAURI_INTERNALS__)
  );
}

/**
 * Check if running in web mode (browser without Tauri)
 * This includes:
 * - Web UI served from embedded server
 * - Standalone web deployment
 */
export function isWebMode(): boolean {
  return !isTauri();
}

/**
 * Check if we're in the desktop app (not just the embedded web UI)
 * The desktop app has additional capabilities like native file dialogs
 */
export function isDesktop(): boolean {
  return isTauri();
}
