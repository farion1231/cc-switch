import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { extractErrorMessage } from "@/utils/errorUtils";

function canUseTauriWindowApis(): boolean {
  return (
    typeof window !== "undefined" &&
    Boolean((window as typeof window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__)
  );
}

/**
 * Tauri 窗口控制：最大化状态同步、装饰切换、最小化/最大化/关闭。
 */
export function useWindowChrome(
  useAppWindowControls: boolean,
  settingsLoaded: boolean,
) {
  const { t } = useTranslation();
  const [isWindowMaximized, setIsWindowMaximized] = useState(false);

  useEffect(() => {
    if (!canUseTauriWindowApis()) return;

    let active = true;
    let unlistenResize: (() => void) | undefined;

    const setupWindowStateSync = async () => {
      try {
        const currentWindow = getCurrentWindow();
        const syncWindowMaximizedState = async () => {
          const maximized = await currentWindow.isMaximized();
          if (active) {
            setIsWindowMaximized(maximized);
          }
        };

        await syncWindowMaximizedState();
        unlistenResize = await currentWindow.onResized(() => {
          void syncWindowMaximizedState();
        });
      } catch (error) {
        console.error("[App] Failed to sync window maximized state", error);
      }
    };

    void setupWindowStateSync();
    return () => {
      active = false;
      unlistenResize?.();
    };
  }, []);

  useEffect(() => {
    // settings 未加载时跳过，避免用 fallback false 覆盖 Rust 侧已设好的装饰状态
    if (!settingsLoaded || !canUseTauriWindowApis()) return;

    const syncWindowDecorations = async () => {
      try {
        await getCurrentWindow().setDecorations(!useAppWindowControls);
      } catch (error) {
        console.error("[App] Failed to update window decorations", error);
      }
    };

    void syncWindowDecorations();
  }, [useAppWindowControls, settingsLoaded]);

  const notifyWindowControlError = useCallback(
    (error: unknown) => {
      toast.error(
        t("notifications.windowControlFailed", {
          defaultValue: "窗口控制失败：{{error}}",
          error: extractErrorMessage(error),
        }),
      );
    },
    [t],
  );

  const minimize = useCallback(async () => {
    if (!canUseTauriWindowApis()) return;

    try {
      await getCurrentWindow().minimize();
    } catch (error) {
      console.error("[App] Failed to minimize window", error);
      notifyWindowControlError(error);
    }
  }, [notifyWindowControlError]);

  const toggleMaximize = useCallback(async () => {
    if (!canUseTauriWindowApis()) return;

    try {
      const currentWindow = getCurrentWindow();
      await currentWindow.toggleMaximize();
      setIsWindowMaximized(await currentWindow.isMaximized());
    } catch (error) {
      console.error("[App] Failed to toggle maximize", error);
      notifyWindowControlError(error);
    }
  }, [notifyWindowControlError]);

  const close = useCallback(async () => {
    if (!canUseTauriWindowApis()) return;

    try {
      await getCurrentWindow().close();
    } catch (error) {
      console.error("[App] Failed to close window", error);
      notifyWindowControlError(error);
    }
  }, [notifyWindowControlError]);

  return { isWindowMaximized, minimize, toggleMaximize, close };
}
