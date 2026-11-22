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
  /** 后台正在下载新版本 */
  autoUpdateDownloading: boolean;
  /** 重新向后端要一次自动更新状态（开关改动后调用） */
  refreshAutoUpdate: () => Promise<void>;
  /** 立刻在后台检查并下载（开关打开时调用）；已在下载就不重复发起 */
  startAutoDownload: () => Promise<void>;
}

const UpdateContext = createContext<UpdateContextValue | undefined>(undefined);

export function UpdateProvider({ children }: { children: React.ReactNode }) {
  const DISMISSED_VERSION_KEY = "ccswitch:update:dismissedVersion";

  const [hasUpdate, setHasUpdate] = useState(false);
  const [updateInfo, setUpdateInfo] = useState<UpdateInfo | null>(null);
  const [isChecking, setIsChecking] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [isDismissed, setIsDismissed] = useState(false);

  // 从 localStorage 读取已关闭的版本
  useEffect(() => {
    const current = updateInfo?.availableVersion;
    if (!current) return;

    setIsDismissed(localStorage.getItem(DISMISSED_VERSION_KEY) === current);
  }, [updateInfo?.availableVersion]);

  const [readyUpdate, setReadyUpdate] = useState<ReadyUpdate | null>(null);
  const [autoUpdateSupported, setAutoUpdateSupported] = useState(false);
  const [routingInUse, setRoutingInUse] = useState(false);
  const [autoUpdateEnabled, setAutoUpdateEnabled] = useState(false);
  const [backendDownloading, setBackendDownloading] = useState(false);
  const [downloadRequested, setDownloadRequested] = useState(false);

  const refreshAutoUpdate = useCallback(async () => {
    try {
      const status = await settingsApi.getAutoUpdateStatus();
      setAutoUpdateSupported(status.supported);
      setAutoUpdateEnabled(status.enabled);
      setBackendDownloading(status.downloading);
      setReadyUpdate(status.ready);
      setRoutingInUse(status.routingInUse);
    } catch (err) {
      console.error("读取自动更新状态失败:", err);
    }
  }, []);

  const downloadInFlightRef = useRef(false);
  const startAutoDownload = useCallback(async () => {
    if (downloadInFlightRef.current) return;
    downloadInFlightRef.current = true;
    setDownloadRequested(true);
    try {
      await settingsApi.downloadAppUpdate();
    } catch (err) {
      console.error("后台下载更新失败:", err);
    } finally {
      downloadInFlightRef.current = false;
      setDownloadRequested(false);
      // 后台循环可能正好也在下（这次调用会立即返回），以后端的下载状态为准
      await refreshAutoUpdate();
    }
  }, [refreshAutoUpdate]);

  // 后台下载在后端进行：挂载时取一次（轻量模式下主窗口会重建），之后听下载完成的事件
  useEffect(() => {
    void refreshAutoUpdate();
    let unlisten: (() => void) | undefined;
    let disposed = false;
    // 载荷为 null：下好的版本被撤回，提示收起
    listen<ReadyUpdate | null>(UPDATE_READY_EVENT, (event) => {
      setReadyUpdate(event.payload ?? null);
      setBackendDownloading(false);
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
        setIsDismissed(
          localStorage.getItem(DISMISSED_VERSION_KEY) ===
            result.info.availableVersion,
        );
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

  // 启动检查发现了新版本、开关开着：马上让后端下载。后端自己的循环要等启动 60 秒后才查，
  // 这段时间关于页只有「更新到 vX」，看上去像开关没生效。每个版本只主动触发一次，
  // 下载失败交给后台循环重试，不跟着重渲染反复联网。
  // 开发构建不主动下（和后端循环一样）：dev 与正式版共用 settings.json，dev 二进制也装不了。
  const availableVersion = updateInfo?.availableVersion;
  const kickedVersionRef = useRef<string | null>(null);
  useEffect(() => {
    if (
      !import.meta.env.DEV &&
      availableVersion &&
      autoUpdateEnabled &&
      autoUpdateSupported &&
      !readyVersion &&
      !backendDownloading &&
      kickedVersionRef.current !== availableVersion
    ) {
      kickedVersionRef.current = availableVersion;
      void startAutoDownload();
    }
  }, [
    availableVersion,
    autoUpdateEnabled,
    autoUpdateSupported,
    readyVersion,
    backendDownloading,
    startAutoDownload,
  ]);

  const dismissUpdate = useCallback(() => {
    setIsDismissed(true);
    if (updateInfo?.availableVersion) {
      localStorage.setItem(DISMISSED_VERSION_KEY, updateInfo.availableVersion);
    }
  }, [updateInfo?.availableVersion]);

  const resetDismiss = useCallback(() => {
    setIsDismissed(false);
    localStorage.removeItem(DISMISSED_VERSION_KEY);
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
    autoUpdateDownloading: downloadRequested || backendDownloading,
    refreshAutoUpdate,
    startAutoDownload,
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
