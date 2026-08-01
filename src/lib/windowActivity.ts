import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";

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

  setWindowActive(document.hasFocus());

  // Browser focus events are a fallback for non-Tauri renderer tests and dev mode.
  window.addEventListener("focus", () => setWindowActive(true));
  window.addEventListener("blur", () => setWindowActive(false));

  if (isTauri()) {
    void getCurrentWindow()
      .onFocusChanged(({ payload }) => setWindowActive(payload))
      .catch((error) => {
        console.error("Failed to observe window focus changes", error);
      });
  }
}
