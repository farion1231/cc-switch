import { useState, useEffect, useCallback } from "react";

const VSCODE_AUTO_SYNC_KEY = "vscode-auto-sync-enabled";
const VSCODE_AUTO_SYNC_EVENT = "vscode-auto-sync-changed";

export function useVSCodeAutoSync() {
  // 默认开启自动同步；若本地存储存在记录，则以记录为准
  const [isAutoSyncEnabled, setIsAutoSyncEnabled] = useState<boolean>(true);

  // 从 localStorage 读取初始状态
  useEffect(() => {
    try {
      const saved = localStorage.getItem(VSCODE_AUTO_SYNC_KEY);
      if (saved !== null) {
        setIsAutoSyncEnabled(saved === "true");
      }
    } catch (error) {
      console.error("读取自动同步状态失败:", error);
    }
  }, []);

  // 订阅同窗口的自定义事件，以及跨窗口的 storage 事件，实现全局同步
  useEffect(() => {
    const onCustom = (e: Event) => {
      try {
        const detail = (e as CustomEvent).detail as
          | { enabled?: boolean }
          | undefined;
        if (detail && typeof detail.enabled === "boolean") {
          setIsAutoSyncEnabled(detail.enabled);
        } else {
          // 兜底：从 localStorage 读取
          const saved = localStorage.getItem(VSCODE_AUTO_SYNC_KEY);
          if (saved !== null) setIsAutoSyncEnabled(saved === "true");
        }
      } catch {
        // 忽略
      }
    };
    const onStorage = (e: StorageEvent) => {
      if (e.key === VSCODE_AUTO_SYNC_KEY) {
        setIsAutoSyncEnabled(e.newValue === "true");
      }
    };
    window.addEventListener(VSCODE_AUTO_SYNC_EVENT, onCustom as EventListener);
    window.addEventListener("storage", onStorage);
    return () => {
      window.removeEventListener(
        VSCODE_AUTO_SYNC_EVENT,
        onCustom as EventListener,
      );
      window.removeEventListener("storage", onStorage);
    };
  }, []);

  // 启用自动同步
  const enableAutoSync = useCallback(() => {
    try {
      localStorage.setItem(VSCODE_AUTO_SYNC_KEY, "true");
      setIsAutoSyncEnabled(true);
      // 通知同窗口其他订阅者
      window.dispatchEvent(
        new CustomEvent(VSCODE_AUTO_SYNC_EVENT, { detail: { enabled: true } }),
      );
    } catch (error) {
      console.error("保存自动同步状态失败:", error);
    }
  }, []);

  // 禁用自动同步
  const disableAutoSync = useCallback(() => {
    try {
      localStorage.setItem(VSCODE_AUTO_SYNC_KEY, "false");
      setIsAutoSyncEnabled(false);
      // 通知同窗口其他订阅者
      window.dispatchEvent(
        new CustomEvent(VSCODE_AUTO_SYNC_EVENT, { detail: { enabled: false } }),
      );
    } catch (error) {
      console.error("保存自动同步状态失败:", error);
    }
  }, []);

  // 切换自动同步状态
  const toggleAutoSync = useCallback(() => {
    if (isAutoSyncEnabled) {
      disableAutoSync();
    } else {
      enableAutoSync();
    }
  }, [isAutoSyncEnabled, enableAutoSync, disableAutoSync]);

  return {
    isAutoSyncEnabled,
    enableAutoSync,
    disableAutoSync,
    toggleAutoSync,
  };
}
