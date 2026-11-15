import React, {
  createContext,
  useContext,
  useState,
  useEffect,
  useCallback,
  useRef,
} from "react";
import { listen } from "@tauri-apps/api/event";
import type { ReadyUpdate, UpdateInfo } from "../lib/updater";
import { UPDATE_READY_EVENT, checkForUpdate } from "../lib/updater";
import { settingsApi } from "../lib/api/settings";
import { extractErrorMessage } from "../utils/errorUtils";

interface UpdateContextValue {
  // 更新状态
  hasUpdate: boolean;
  updateInfo: UpdateInfo | null;
  isChecking: boolean;
  error: string | null;

  // 提示状态
  isDismissed: boolean;
  dismissUpdate: () => void;

  // 操作方法
  checkUpdate: () => Promise<boolean>;
  resetDismiss: () => void;

  // 自动更新（下载状态在后端，主窗口重建后从后端取回）
  /** 后台已下好、等确认安装的版本 */
  readyUpdate: ReadyUpdate | null;
  /** 这台机器能不能自动安装（便携版、Linux 非 AppImage 不行） */
  autoUpdateSupported: boolean;
  /** 有应用在用路由服务（重启会切断进行中的请求），随 refreshAutoUpdate 更新 */
  routingInUse: boolean;
  /** 重新向后端要一次自动更新状态（开关改动后调用） */
  refreshAutoUpdate: () => Promise<void>;
}

const UpdateContext = createContext<UpdateContextValue | undefined>(undefined);

export function UpdateProvider({ children }: { children: React.ReactNode }) {
  const DISMISSED_VERSION_KEY = "ccswitch:update:dismissedVersion";
  const LEGACY_DISMISSED_KEY = "dismissedUpdateVersion"; // 兼容旧键

  const [hasUpdate, setHasUpdate] = useState(false);
  const [updateInfo, setUpdateInfo] = useState<UpdateInfo | null>(null);
  const [isChecking, setIsChecking] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [isDismissed, setIsDismissed] = useState(false);

  // 从 localStorage 读取已关闭的版本
  useEffect(() => {
    const current = updateInfo?.availableVersion;
    if (!current) return;

    // 读取新键；若不存在，尝试迁移旧键
    let dismissedVersion = localStorage.getItem(DISMISSED_VERSION_KEY);
    if (!dismissedVersion) {
      const legacy = localStorage.getItem(LEGACY_DISMISSED_KEY);
      if (legacy) {
        localStorage.setItem(DISMISSED_VERSION_KEY, legacy);
        localStorage.removeItem(LEGACY_DISMISSED_KEY);
        dismissedVersion = legacy;
      }
    }

    setIsDismissed(dismissedVersion === current);
  }, [updateInfo?.availableVersion]);

  const [readyUpdate, setReadyUpdate] = useState<ReadyUpdate | null>(null);
  const [autoUpdateSupported, setAutoUpdateSupported] = useState(false);
  const [routingInUse, setRoutingInUse] = useState(false);

  const refreshAutoUpdate = useCallback(async () => {
    try {
      const status = await settingsApi.getAutoUpdateStatus();
      setAutoUpdateSupported(status.supported);
      setReadyUpdate(status.ready);
      setRoutingInUse(status.routingInUse);
    } catch (err) {
      console.error("读取自动更新状态失败:", err);
    }
  }, []);

  // 后台下载在后端进行：挂载时取一次（轻量模式下主窗口会重建），之后听下载完成的事件
  useEffect(() => {
    void refreshAutoUpdate();
    let unlisten: (() => void) | undefined;
    let disposed = false;
    // 载荷为 null：下好的版本被撤回，提示收起
    listen<ReadyUpdate | null>(UPDATE_READY_EVENT, (event) => {
      setReadyUpdate(event.payload ?? null);
    })
      .then((fn) => {
        if (disposed) fn();
        else unlisten = fn;
      })
      .catch((err) => console.error("监听自动更新事件失败:", err));
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [refreshAutoUpdate]);

  const isCheckingRef = useRef(false);

  const checkUpdate = useCallback(async () => {
    if (isCheckingRef.current) return false;
    isCheckingRef.current = true;
    setIsChecking(true);
    setError(null);

    try {
      const result = await checkForUpdate({ timeout: 30000 });

      if (result.status === "available") {
        setHasUpdate(true);
        setUpdateInfo(result.info);

        // 检查是否已经关闭过这个版本的提醒
        let dismissedVersion = localStorage.getItem(DISMISSED_VERSION_KEY);
        if (!dismissedVersion) {
          const legacy = localStorage.getItem(LEGACY_DISMISSED_KEY);
          if (legacy) {
            localStorage.setItem(DISMISSED_VERSION_KEY, legacy);
            localStorage.removeItem(LEGACY_DISMISSED_KEY);
            dismissedVersion = legacy;
          }
        }
        setIsDismissed(dismissedVersion === result.info.availableVersion);
        return true; // 有更新
      } else {
        setHasUpdate(false);
        setUpdateInfo(null);
        setIsDismissed(false);
        return false; // 已是最新
      }
    } catch (err) {
      console.error("检查更新失败:", err);
      setError(extractErrorMessage(err) || "检查更新失败");
      setHasUpdate(false);
      throw err; // 抛出错误让调用方处理
    } finally {
      setIsChecking(false);
      isCheckingRef.current = false;
    }
  }, []);

  // 后台下好了新版本，但启动时那次检查没看到（应用开着期间才发版）：补查一次，
  // 让更新说明跟上。每个下好的版本只补查一次：安装提示每 5 秒轮询一次状态，
  // 离线时补查会一直失败，不能跟着轮询反复联网。
  const readyVersion = readyUpdate?.version;
  const recheckedVersionRef = useRef<string | null>(null);
  useEffect(() => {
    if (
      readyVersion &&
      updateInfo?.availableVersion !== readyVersion &&
      recheckedVersionRef.current !== readyVersion
    ) {
      recheckedVersionRef.current = readyVersion;
      checkUpdate().catch(console.error);
    }
  }, [readyVersion, updateInfo?.availableVersion, checkUpdate]);

  const dismissUpdate = useCallback(() => {
    setIsDismissed(true);
    if (updateInfo?.availableVersion) {
      localStorage.setItem(DISMISSED_VERSION_KEY, updateInfo.availableVersion);
      // 清理旧键
      localStorage.removeItem(LEGACY_DISMISSED_KEY);
    }
  }, [updateInfo?.availableVersion]);

  const resetDismiss = useCallback(() => {
    setIsDismissed(false);
    localStorage.removeItem(DISMISSED_VERSION_KEY);
    localStorage.removeItem(LEGACY_DISMISSED_KEY);
  }, []);

  // 应用启动时自动检查更新
  useEffect(() => {
    // 延迟1秒后检查，避免影响启动体验
    const timer = setTimeout(() => {
      checkUpdate().catch(console.error);
    }, 1000);

    return () => clearTimeout(timer);
  }, [checkUpdate]);

  // 后端手里有下好的包，就算有更新：离线、或主窗口重建后联网检查还没成功，
  // 侧栏红点和关于页的安装按钮也要在（安装命令离线时会直接装缓存的包）
  const effectiveUpdateInfo: UpdateInfo | null =
    updateInfo ??
    (readyUpdate
      ? {
          currentVersion: "",
          availableVersion: readyUpdate.version,
          notes: readyUpdate.notes ?? undefined,
        }
      : null);

  const value: UpdateContextValue = {
    hasUpdate: hasUpdate || !!readyUpdate,
    updateInfo: effectiveUpdateInfo,
    isChecking,
    error,
    isDismissed,
    dismissUpdate,
    checkUpdate,
    resetDismiss,
    readyUpdate,
    autoUpdateSupported,
    routingInUse,
    refreshAutoUpdate,
  };

  return (
    <UpdateContext.Provider value={value}>{children}</UpdateContext.Provider>
  );
}

export function useUpdate() {
  const context = useContext(UpdateContext);
  if (!context) {
    throw new Error("useUpdate must be used within UpdateProvider");
  }
  return context;
}
