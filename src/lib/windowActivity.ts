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

  // Visible monitoring windows must keep refreshing while the user works in a
  // terminal. Only hidden/minimized windows pause Query; focus controls the
  // decorative heartbeat. Native visibility covers WebViews that remain
  // document-visible after being hidden.
  focusManager.setEventListener((handleFocus) => {
    const currentWindow = isTauri() ? getCurrentWindow() : undefined;
    let focused = document.hasFocus();
    let nativeVisible = true;
    let focusRevision = 0;
    let visibilityRevision = 0;
    let disposed = false;
    const unlisteners: (() => void)[] = [];
    const update = () => {
      const visible = nativeVisible && document.visibilityState !== "hidden";
      setWindowActive(focused && visible);
      handleFocus(visible);
    };
    const refreshVisibility = async () => {
      if (!currentWindow || disposed) return;
      const revision = ++visibilityRevision;
      try {
        const [visible, minimized] = await Promise.all([
          currentWindow.isVisible(),
          currentWindow.isMinimized(),
        ]);
        if (disposed || visibilityRevision !== revision) return;
        nativeVisible = visible && !minimized;
        update();
      } catch (error) {
        console.error("Failed to read window visibility", error);
      }
    };
    const onVisibilityChange = () => {
      update();
      void refreshVisibility();
    };
    const onFocus = () => {
      focusRevision += 1;
      focused = true;
      onVisibilityChange();
    };
    const onBlur = () => {
      focusRevision += 1;
      focused = false;
      onVisibilityChange();
    };
    window.addEventListener("focus", onFocus);
    window.addEventListener("blur", onBlur);
    document.addEventListener("visibilitychange", onVisibilityChange);
    update();

    if (currentWindow) {
      const observe = async (register: () => Promise<() => void>) => {
        try {
          const off = await register();
          if (disposed) off();
          else unlisteners.push(off);
        } catch (error) {
          console.error("Failed to observe window activity", error);
        }
      };
      void (async () => {
        await Promise.all([
          observe(() =>
            currentWindow.onFocusChanged(({ payload }) => {
              if (disposed) return;
              focusRevision += 1;
              focused = payload;
              onVisibilityChange();
            }),
          ),
          observe(() =>
            currentWindow.onResized(() => void refreshVisibility()),
          ),
          // Closing an already-unfocused window to the tray may emit no blur
          // or resize. The backend notifies after its explicit show/hide calls.
          observe(() =>
            currentWindow.listen("main-window-visibility-changed", () => {
              void refreshVisibility();
            }),
          ),
        ]);
        if (disposed) return;
        // Register before snapshots so transitions during setup cannot be lost.
        // Visibility must not wait for the separate decorative focus snapshot.
        void refreshVisibility();
        const revision = focusRevision;
        try {
          const active = await currentWindow.isFocused();
          if (disposed || focusRevision !== revision) return;
          focused = active;
          update();
        } catch (error) {
          console.error("Failed to read initial window focus", error);
        }
      })();
    }

    return () => {
      disposed = true;
      window.removeEventListener("focus", onFocus);
      window.removeEventListener("blur", onBlur);
      document.removeEventListener("visibilitychange", onVisibilityChange);
      unlisteners.forEach((off) => off());
      stopHeartbeat();
      delete document.documentElement.dataset.windowActive;
    };
  });
}
