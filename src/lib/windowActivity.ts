import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { focusManager } from "@tanstack/react-query";

const HEARTBEAT_INTERVAL_MS = 3000;
const HEARTBEAT_DIM_MS = 300;

let initialized = false;
let heartbeatInterval: number | undefined;
let heartbeatReset: number | undefined;

function stopHeartbeat() {
  if (heartbeatInterval !== undefined) {
    window.clearInterval(heartbeatInterval);
    heartbeatInterval = undefined;
  }
  if (heartbeatReset !== undefined) {
    window.clearTimeout(heartbeatReset);
    heartbeatReset = undefined;
  }
  delete document.documentElement.dataset.statusHeartbeat;
}

function startHeartbeat() {
  stopHeartbeat();
  heartbeatInterval = window.setInterval(() => {
    document.documentElement.dataset.statusHeartbeat = "true";
    heartbeatReset = window.setTimeout(() => {
      delete document.documentElement.dataset.statusHeartbeat;
      heartbeatReset = undefined;
    }, HEARTBEAT_DIM_MS);
  }, HEARTBEAT_INTERVAL_MS);
}

function setWindowActive(active: boolean) {
  if (document.documentElement.dataset.windowActive === String(active)) return;
  document.documentElement.dataset.windowActive = String(active);

  if (active) {
    startHeartbeat();
  } else {
    stopHeartbeat();
  }
}

export function initializeWindowActivity() {
  if (initialized) return;
  initialized = true;

  // Desktop WebViews may remain document-visible after being hidden/minimized.
  // Use native focus as well as DOM visibility for Query's foreground polling,
  // session auto-sync, and the decorative heartbeat. Explicit background quota
  // queries retain their refetchIntervalInBackground behavior.
  focusManager.setEventListener((handleFocus) => {
    let focused = document.hasFocus();
    let focusRevision = 0;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    const update = () => {
      const active = focused && document.visibilityState !== "hidden";
      setWindowActive(active);
      handleFocus(active);
    };
    const onFocus = () => {
      focusRevision += 1;
      focused = true;
      update();
    };
    const onBlur = () => {
      focusRevision += 1;
      focused = false;
      update();
    };
    window.addEventListener("focus", onFocus);
    window.addEventListener("blur", onBlur);
    document.addEventListener("visibilitychange", update);
    update();

    if (isTauri()) {
      const currentWindow = getCurrentWindow();
      void currentWindow
        .onFocusChanged(({ payload }) => {
          if (disposed) return;
          focusRevision += 1;
          focused = payload;
          update();
        })
        .then((off) => {
          if (disposed) off();
          else unlisten = off;
        })
        .catch((error) =>
          console.error("Failed to observe window focus changes", error),
        );
      // The window can already be focused before its listener is registered.
      // Never let a delayed initial snapshot overwrite a newer focus event.
      const revision = focusRevision;
      void currentWindow
        .isFocused()
        .then((active) => {
          if (disposed || focusRevision !== revision) return;
          focused = active;
          update();
        })
        .catch((error) =>
          console.error("Failed to read initial window focus", error),
        );
    }

    return () => {
      disposed = true;
      window.removeEventListener("focus", onFocus);
      window.removeEventListener("blur", onBlur);
      document.removeEventListener("visibilitychange", update);
      unlisten?.();
      stopHeartbeat();
      delete document.documentElement.dataset.windowActive;
    };
  });
}
