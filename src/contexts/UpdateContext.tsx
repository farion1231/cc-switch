import React, {
  createContext,
  useContext,
  useState,
  useEffect,
  useCallback,
  useRef,
} from "react";
import { useTranslation } from "react-i18next";
import type { UpdateInfo } from "../lib/updater";
import { checkForUpdate } from "../lib/updater";
import { settingsApi } from "../lib/api";
import { useSettingsQuery } from "../lib/query";
import { toast } from "../lib/toast";
import { extractErrorMessage } from "../utils/errorUtils";

/** 发现更新后留给用户点取消的时间，到点才安装并重启。 */
const SILENT_UPDATE_DELAY_MS = 8000;
/** 比等待更长，取消按钮在倒计时期间一直看得见。 */
const SILENT_UPDATE_TOAST_MS = 12000;
const UPDATE_ALREADY_IN_PROGRESS = "update-already-in-progress";

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
}

const UpdateContext = createContext<UpdateContextValue | undefined>(undefined);

export function UpdateProvider({ children }: { children: React.ReactNode }) {
  const DISMISSED_VERSION_KEY = "ccswitch:update:dismissedVersion";
  const LEGACY_DISMISSED_KEY = "dismissedUpdateVersion"; // 兼容旧键

  const { t } = useTranslation();
  const { data: settings } = useSettingsQuery();
  // 查询还没返回时 data 是 undefined。不能把「还在加载」当成已开启。
  const settingsLoaded = settings !== undefined;
  // 缺省开启；只有设置里写了 false 才关闭。
  const silentAppUpdate =
    settings !== undefined && settings.silentAppUpdate !== false;

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

  const isCheckingRef = useRef(false);
  // 本会话已经开始安装的版本。取消不写这里，下次启动还能再试。
  const installingVersionRef = useRef<string | null>(null);
  const availableVersion =
    hasUpdate && updateInfo?.availableVersion
      ? updateInfo.availableVersion
      : "";

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

  // 启动检查发现更新后自动安装。依赖拆成稳定值，设置重新拉取不会把 8 秒倒计时打断重来。
  useEffect(() => {
    if (
      !settingsLoaded ||
      !silentAppUpdate ||
      !availableVersion ||
      isDismissed ||
      installingVersionRef.current === availableVersion
    ) {
      return;
    }

    let cancelled = false;
    let timer: ReturnType<typeof setTimeout> | undefined;

    const begin = async () => {
      let portable = true;
      try {
        portable = await settingsApi.isPortable();
      } catch (error) {
        console.error(
          "[UpdateProvider] 无法确认是否便携模式，跳过静默更新",
          error,
        );
        return;
      }
      if (cancelled || portable) return;
      if (installingVersionRef.current === availableVersion) return;

      const cancelInstall = () => {
        cancelled = true;
        if (timer !== undefined) {
          clearTimeout(timer);
          timer = undefined;
        }
      };

      toast.info(
        t("settings.silentUpdateStarting", { version: availableVersion }),
        {
          duration: SILENT_UPDATE_TOAST_MS,
          action: {
            label: t("common.cancel"),
            onClick: cancelInstall,
          },
        },
      );

      timer = setTimeout(() => {
        if (cancelled) return;
        installingVersionRef.current = availableVersion;
        void (async () => {
          try {
            const installed = await settingsApi.installUpdateAndRestart();
            if (
              !installed &&
              installingVersionRef.current === availableVersion
            ) {
              installingVersionRef.current = null;
            }
          } catch (error) {
            const message = extractErrorMessage(error);
            if (message.includes(UPDATE_ALREADY_IN_PROGRESS)) return;
            if (installingVersionRef.current === availableVersion) {
              installingVersionRef.current = null;
            }
            toast.error(t("settings.updateFailed"), {
              description: message || undefined,
            });
          }
        })();
      }, SILENT_UPDATE_DELAY_MS);
    };

    void begin();

    return () => {
      cancelled = true;
      if (timer !== undefined) {
        clearTimeout(timer);
      }
    };
  }, [availableVersion, silentAppUpdate, settingsLoaded, isDismissed, t]);

  const value: UpdateContextValue = {
    hasUpdate,
    updateInfo,
    isChecking,
    error,
    isDismissed,
    dismissUpdate,
    checkUpdate,
    resetDismiss,
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
