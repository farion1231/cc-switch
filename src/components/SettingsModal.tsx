import { useState, useEffect } from "react";
import { useTranslation } from "react-i18next";
import {
  X,
  RefreshCw,
  FolderOpen,
  Download,
  ExternalLink,
  Check,
  Undo2,
  FolderSearch,
  Save,
} from "lucide-react";
import { getVersion } from "@tauri-apps/api/app";
import { ImportProgressModal } from "./ImportProgressModal";
import { homeDir, join } from "@tauri-apps/api/path";
import "../lib/tauri-api";
import { relaunchApp } from "../lib/updater";
import { useUpdate } from "../contexts/UpdateContext";
import type { Settings } from "../types";
import type { AppType } from "../lib/tauri-api";
import { isLinux } from "../lib/platform";

interface SettingsModalProps {
  onClose: () => void;
  onImportSuccess?: () => void | Promise<void>;
  onNotify?: (
    message: string,
    type: "success" | "error",
    duration?: number,
  ) => void;
}

export default function SettingsModal({
  onClose,
  onImportSuccess,
  onNotify,
}: SettingsModalProps) {
  const { t, i18n } = useTranslation();

  const normalizeLanguage = (lang?: string | null): "zh" | "en" =>
    lang === "en" ? "en" : "zh";

  const readPersistedLanguage = (): "zh" | "en" => {
    if (typeof window !== "undefined") {
      const stored = window.localStorage.getItem("language");
      if (stored === "en" || stored === "zh") {
        return stored;
      }
    }
    return normalizeLanguage(i18n.language);
  };

  const persistedLanguage = readPersistedLanguage();

  const [settings, setSettings] = useState<Settings>({
    showInTray: true,
    minimizeToTrayOnClose: true,
    enableClaudePluginIntegration: false,
    claudeConfigDir: undefined,
    codexConfigDir: undefined,
    language: persistedLanguage,
  });
  // appConfigDir 现在从 Store 独立管理
  const [appConfigDir, setAppConfigDir] = useState<string | undefined>(
    undefined,
  );
  const [initialLanguage, setInitialLanguage] = useState<"zh" | "en">(
    persistedLanguage,
  );
  const [configPath, setConfigPath] = useState<string>("");
  const [version, setVersion] = useState<string>("");
  const [isCheckingUpdate, setIsCheckingUpdate] = useState(false);
  const [isDownloading, setIsDownloading] = useState(false);
  const [showUpToDate, setShowUpToDate] = useState(false);
  const [resolvedAppConfigDir, setResolvedAppConfigDir] = useState<string>("");
  const [resolvedClaudeDir, setResolvedClaudeDir] = useState<string>("");
  const [resolvedCodexDir, setResolvedCodexDir] = useState<string>("");
  const [isPortable, setIsPortable] = useState(false);
  const [initialAppConfigDir, setInitialAppConfigDir] = useState<
    string | undefined
  >(undefined);
  const [showRestartDialog, setShowRestartDialog] = useState(false);
  const { hasUpdate, updateInfo, updateHandle, checkUpdate, resetDismiss } =
    useUpdate();

  // 导入/导出相关状态
  const [isImporting, setIsImporting] = useState(false);
  const [importStatus, setImportStatus] = useState<
    "idle" | "importing" | "success" | "error"
  >("idle");
  const [importError, setImportError] = useState<string>("");
  const [importBackupId, setImportBackupId] = useState<string>("");
  const [selectedImportFile, setSelectedImportFile] = useState<string>("");

  useEffect(() => {
    loadSettings();
    loadAppConfigDirFromStore(); // 从 Store 加载 appConfigDir
    loadConfigPath();
    loadVersion();
    loadResolvedDirs();
    loadPortableFlag();
  }, []);

  const loadVersion = async () => {
    try {
      const appVersion = await getVersion();
      setVersion(appVersion);
    } catch (error) {
      console.error(t("console.getVersionFailed"), error);
      // 失败时不硬编码版本号，显示为未知
      setVersion(t("common.unknown"));
    }
  };

  // 从 Tauri Store 加载 appConfigDir
  const loadAppConfigDirFromStore = async () => {
    try {
      const storeValue = await (window as any).api.getAppConfigDirOverride();
      if (storeValue) {
        setAppConfigDir(storeValue);
        setInitialAppConfigDir(storeValue);
        setResolvedAppConfigDir(storeValue);
      } else {
        // 使用默认值
        const defaultDir = await computeDefaultAppConfigDir();
        setResolvedAppConfigDir(defaultDir);
      }
    } catch (error) {
      console.error("从 Store 加载 appConfigDir 失败:", error);
    }
  };

  const loadSettings = async () => {
    try {
      const loadedSettings = await window.api.getSettings();
      const showInTray =
        (loadedSettings as any)?.showInTray ??
        (loadedSettings as any)?.showInDock ??
        true;
      const minimizeToTrayOnClose =
        (loadedSettings as any)?.minimizeToTrayOnClose ??
        (loadedSettings as any)?.minimize_to_tray_on_close ??
        true;
      const storedLanguage = normalizeLanguage(
        typeof (loadedSettings as any)?.language === "string"
          ? (loadedSettings as any).language
          : persistedLanguage,
      );

      setSettings({
        showInTray,
        minimizeToTrayOnClose,
        enableClaudePluginIntegration:
          typeof (loadedSettings as any)?.enableClaudePluginIntegration ===
          "boolean"
            ? (loadedSettings as any).enableClaudePluginIntegration
            : false,
        claudeConfigDir:
          typeof (loadedSettings as any)?.claudeConfigDir === "string"
            ? (loadedSettings as any).claudeConfigDir
            : undefined,
        codexConfigDir:
          typeof (loadedSettings as any)?.codexConfigDir === "string"
            ? (loadedSettings as any).codexConfigDir
            : undefined,
        language: storedLanguage,
      });
      setInitialLanguage(storedLanguage);
      if (i18n.language !== storedLanguage) {
        void i18n.changeLanguage(storedLanguage);
      }
    } catch (error) {
      console.error(t("console.loadSettingsFailed"), error);
    }
  };

  const loadConfigPath = async () => {
    try {
      const path = await window.api.getAppConfigPath();
      if (path) {
        setConfigPath(path);
      }
    } catch (error) {
      console.error(t("console.getConfigPathFailed"), error);
    }
  };

  const loadResolvedDirs = async () => {
    try {
      const [claudeDir, codexDir] = await Promise.all([
        window.api.getConfigDir("claude"),
        window.api.getConfigDir("codex"),
      ]);
      setResolvedClaudeDir(claudeDir || "");
      setResolvedCodexDir(codexDir || "");
    } catch (error) {
      console.error(t("console.getConfigDirFailed"), error);
    }
  };

  const loadPortableFlag = async () => {
    try {
      const portable = await window.api.isPortable();
      setIsPortable(portable);
    } catch (error) {
      console.error(t("console.detectPortableFailed"), error);
    }
  };

  const saveSettings = async () => {
    try {
      const selectedLanguage = settings.language === "en" ? "en" : "zh";
      const payload: Settings = {
        ...settings,
        claudeConfigDir:
          settings.claudeConfigDir && settings.claudeConfigDir.trim() !== ""
            ? settings.claudeConfigDir.trim()
            : undefined,
        codexConfigDir:
          settings.codexConfigDir && settings.codexConfigDir.trim() !== ""
            ? settings.codexConfigDir.trim()
            : undefined,
        language: selectedLanguage,
      };

      // 保存 settings.json (不包含 appConfigDir)
      await window.api.saveSettings(payload);

      // 单独保存 appConfigDir 到 Store
      const normalizedAppConfigDir =
        appConfigDir && appConfigDir.trim() !== ""
          ? appConfigDir.trim()
          : null;
      await (window as any).api.setAppConfigDirOverride(normalizedAppConfigDir);

      // 立即生效：根据开关无条件写入/移除 ~/.claude/config.json
      try {
        if (payload.enableClaudePluginIntegration) {
          await window.api.applyClaudePluginConfig({ official: false });
        } else {
          await window.api.applyClaudePluginConfig({ official: true });
        }
      } catch (e) {
        console.warn("[Settings] Apply Claude plugin config on save failed", e);
      }

      // 检测 appConfigDir 是否真正发生变化
      const appConfigDirChanged =
        (normalizedAppConfigDir || undefined) !==
        (initialAppConfigDir || undefined);

      setSettings(payload);
      setInitialAppConfigDir(normalizedAppConfigDir ?? undefined);
      try {
        window.localStorage.setItem("language", selectedLanguage);
      } catch (error) {
        console.warn("[Settings] Failed to persist language preference", error);
      }
      setInitialLanguage(selectedLanguage);
      if (i18n.language !== selectedLanguage) {
        void i18n.changeLanguage(selectedLanguage);
      }

      // 如果修改了 appConfigDir,需要提示用户重启应用程序
      if (appConfigDirChanged) {
        setShowRestartDialog(true);
      } else {
        onClose();
      }
    } catch (error) {
      console.error(t("console.saveSettingsFailed"), error);
    }
  };

  const handleRestartNow = async () => {
    // 开发模式下不真正重启,只提示
    if (import.meta.env.DEV) {
      onNotify?.(
        t("settings.devModeRestartHint"),
        "success",
        5000,
      );
      setShowRestartDialog(false);
      onClose();
      return;
    }

    // 生产模式下真正重启应用
    try {
      await window.api.restartApp();
    } catch (e) {
      console.warn("[Settings] Restart app failed", e);
      // 如果重启失败，仍然关闭设置窗口
      setShowRestartDialog(false);
      onClose();
    }
  };

  const handleRestartLater = () => {
    setShowRestartDialog(false);
    onClose();
  };

  const handleLanguageChange = (lang: "zh" | "en") => {
    setSettings((prev) => ({ ...prev, language: lang }));
    if (i18n.language !== lang) {
      void i18n.changeLanguage(lang);
    }
  };

  const handleCancel = () => {
    if (settings.language !== initialLanguage) {
      setSettings((prev) => ({ ...prev, language: initialLanguage }));
      if (i18n.language !== initialLanguage) {
        void i18n.changeLanguage(initialLanguage);
      }
    }
    onClose();
  };

  const handleCheckUpdate = async () => {
    if (hasUpdate && updateHandle) {
      if (isPortable) {
        await window.api.checkForUpdates();
        return;
      }
      // 已检测到更新：直接复用 updateHandle 下载并安装，避免重复检查
      setIsDownloading(true);
      try {
        resetDismiss();
        await updateHandle.downloadAndInstall();
        await relaunchApp();
      } catch (error) {
        console.error(t("console.updateFailed"), error);
        // 更新失败时回退到打开 Releases 页面
        await window.api.checkForUpdates();
      } finally {
        setIsDownloading(false);
      }
    } else {
      // 尚未检测到更新：先检查
      setIsCheckingUpdate(true);
      setShowUpToDate(false);
      try {
        const hasNewUpdate = await checkUpdate();
        // 检查完成后，如果没有更新，显示"已是最新"
        if (!hasNewUpdate) {
          setShowUpToDate(true);
          // 3秒后恢复按钮文字
          setTimeout(() => {
            setShowUpToDate(false);
          }, 3000);
        }
      } catch (error) {
        console.error(t("console.checkUpdateFailed"), error);
        // 在开发模式下，模拟已是最新版本的响应
        if (import.meta.env.DEV) {
          setShowUpToDate(true);
          setTimeout(() => {
            setShowUpToDate(false);
          }, 3000);
        } else {
          // 生产环境下如果更新插件不可用，回退到打开 Releases 页面
          await window.api.checkForUpdates();
        }
      } finally {
        setIsCheckingUpdate(false);
      }
    }
  };

  const handleOpenConfigFolder = async () => {
    try {
      await window.api.openAppConfigFolder();
    } catch (error) {
      console.error(t("console.openConfigFolderFailed"), error);
    }
  };

  const handleBrowseAppConfigDir = async () => {
    try {
      const currentResolved = appConfigDir ?? resolvedAppConfigDir;
      const selected = await window.api.selectConfigDirectory(currentResolved);

      if (!selected) {
        return;
      }

      const sanitized = selected.trim();

      if (sanitized === "") {
        return;
      }

      setAppConfigDir(sanitized);
      setResolvedAppConfigDir(sanitized);
    } catch (error) {
      console.error(t("console.selectConfigDirFailed"), error);
    }
  };

  const handleBrowseConfigDir = async (app: AppType) => {
    try {
      const currentResolved =
        app === "claude"
          ? (settings.claudeConfigDir ?? resolvedClaudeDir)
          : (settings.codexConfigDir ?? resolvedCodexDir);

      const selected = await window.api.selectConfigDirectory(currentResolved);

      if (!selected) {
        return;
      }

      const sanitized = selected.trim();

      if (sanitized === "") {
        return;
      }

      if (app === "claude") {
        setSettings((prev) => ({ ...prev, claudeConfigDir: sanitized }));
        setResolvedClaudeDir(sanitized);
      } else {
        setSettings((prev) => ({ ...prev, codexConfigDir: sanitized }));
        setResolvedCodexDir(sanitized);
      }
    } catch (error) {
      console.error(t("console.selectConfigDirFailed"), error);
    }
  };

  const computeDefaultConfigDir = async (app: AppType) => {
    try {
      const home = await homeDir();
      const folder = app === "claude" ? ".claude" : ".codex";
      return await join(home, folder);
    } catch (error) {
      console.error(t("console.getDefaultConfigDirFailed"), error);
      return "";
    }
  };

  const computeDefaultAppConfigDir = async () => {
    try {
      const home = await homeDir();
      return await join(home, ".cc-switch");
    } catch (error) {
      console.error(t("console.getDefaultConfigDirFailed"), error);
      return "";
    }
  };

  const handleResetAppConfigDir = async () => {
    setAppConfigDir(undefined);
    const defaultDir = await computeDefaultAppConfigDir();
    if (defaultDir) {
      setResolvedAppConfigDir(defaultDir);
    }
  };

  const handleResetConfigDir = async (app: AppType) => {
    setSettings((prev) => ({
      ...prev,
      ...(app === "claude"
        ? { claudeConfigDir: undefined }
        : { codexConfigDir: undefined }),
    }));

    const defaultDir = await computeDefaultConfigDir(app);
    if (!defaultDir) {
      return;
    }

    if (app === "claude") {
      setResolvedClaudeDir(defaultDir);
    } else {
      setResolvedCodexDir(defaultDir);
    }
  };

  const handleOpenReleaseNotes = async () => {
    try {
      const targetVersion = updateInfo?.availableVersion || version;
      const unknownLabel = t("common.unknown");
      // 如果未知或为空，回退到 releases 首页
      if (!targetVersion || targetVersion === unknownLabel) {
        await window.api.openExternal(
          "https://github.com/farion1231/cc-switch/releases",
        );
        return;
      }
      const tag = targetVersion.startsWith("v")
        ? targetVersion
        : `v${targetVersion}`;
      await window.api.openExternal(
        `https://github.com/farion1231/cc-switch/releases/tag/${tag}`,
      );
    } catch (error) {
      console.error(t("console.openReleaseNotesFailed"), error);
    }
  };

  // 导出配置处理函数
  const handleExportConfig = async () => {
    try {
      const defaultName = `cc-switch-config-${new Date().toISOString().split("T")[0]}.json`;
      const filePath = await window.api.saveFileDialog(defaultName);

      if (!filePath) {
        onNotify?.(
          `${t("settings.exportFailed")}: ${t("settings.selectFileFailed")}`,
          "error",
          4000,
        );
        return;
      }

      const result = await window.api.exportConfigToFile(filePath);

      if (result.success) {
        onNotify?.(
          `${t("settings.configExported")}\n${result.filePath}`,
          "success",
          4000,
        );
      }
    } catch (error) {
      console.error(t("settings.exportFailedError"), error);
      onNotify?.(
        `${t("settings.exportFailed")}: ${String(error)}`,
        "error",
        5000,
      );
    }
  };

  // 选择要导入的文件
  const handleSelectImportFile = async () => {
    try {
      const filePath = await window.api.openFileDialog();
      if (filePath) {
        setSelectedImportFile(filePath);
        setImportStatus("idle"); // 重置状态
        setImportError("");
      }
    } catch (error) {
      console.error(t("settings.selectFileFailed") + ":", error);
      onNotify?.(
        `${t("settings.selectFileFailed")}: ${String(error)}`,
        "error",
        5000,
      );
    }
  };

  // 执行导入
  const handleExecuteImport = async () => {
    if (!selectedImportFile || isImporting) return;

    setIsImporting(true);
    setImportStatus("importing");

    try {
      const result = await window.api.importConfigFromFile(selectedImportFile);

      if (result.success) {
        setImportBackupId(result.backupId || "");
        setImportStatus("success");
        // ImportProgressModal 会在2秒后触发数据刷新回调
      } else {
        setImportError(result.message || t("settings.configCorrupted"));
        setImportStatus("error");
      }
    } catch (error) {
      setImportError(String(error));
      setImportStatus("error");
    } finally {
      setIsImporting(false);
    }
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) handleCancel();
      }}
    >
      <div
        className={`absolute inset-0 bg-black/50 dark:bg-black/70${
          isLinux() ? "" : " backdrop-blur-sm"
        }`}
      />
      <div className="relative bg-white dark:bg-gray-900 rounded-xl shadow-2xl w-[500px] max-h-[90vh] flex flex-col overflow-hidden">
        {/* 标题栏 */}
        <div className="flex items-center justify-between px-6 py-4 border-b border-gray-200 dark:border-gray-800">
          <h2 className="text-lg font-semibold text-blue-500 dark:text-blue-400">
            {t("settings.title")}
          </h2>
          <button
            onClick={handleCancel}
            className="p-1.5 hover:bg-gray-100 dark:hover:bg-gray-800 rounded-md transition-colors"
          >
            <X size={20} className="text-gray-500 dark:text-gray-400" />
          </button>
        </div>

        {/* 设置内容 */}
        <div className="px-6 py-4 space-y-6 overflow-y-auto flex-1">
          {/* 语言设置 */}
          <div>
            <h3 className="text-sm font-medium text-gray-900 dark:text-gray-100 mb-3">
              {t("settings.language")}
            </h3>
            <div className="inline-flex p-0.5 bg-gray-100 dark:bg-gray-800 rounded-lg">
              <button
                type="button"
                onClick={() => handleLanguageChange("zh")}
                className={`px-4 py-1.5 text-sm font-medium rounded-md transition-all min-w-[80px] ${
                  (settings.language ?? "zh") === "zh"
                    ? "bg-white dark:bg-gray-700 text-gray-900 dark:text-gray-100 shadow-sm"
                    : "text-gray-600 dark:text-gray-400 hover:text-gray-900 dark:hover:text-gray-200"
                }`}
              >
                {t("settings.languageOptionChinese")}
              </button>
              <button
                type="button"
                onClick={() => handleLanguageChange("en")}
                className={`px-4 py-1.5 text-sm font-medium rounded-md transition-all min-w-[80px] ${
                  settings.language === "en"
                    ? "bg-white dark:bg-gray-700 text-gray-900 dark:text-gray-100 shadow-sm"
                    : "text-gray-600 dark:text-gray-400 hover:text-gray-900 dark:hover:text-gray-200"
                }`}
              >
                {t("settings.languageOptionEnglish")}
              </button>
            </div>
          </div>

          {/* 窗口行为设置 */}
          <div>
            <h3 className="text-sm font-medium text-gray-900 dark:text-gray-100 mb-3">
              {t("settings.windowBehavior")}
            </h3>
            <div className="space-y-3">
              <label className="flex items-center justify-between">
                <div>
                  <span className="text-sm text-gray-900 dark:text-gray-100">
                    {t("settings.minimizeToTray")}
                  </span>
                  <p className="text-xs text-gray-500 dark:text-gray-400 mt-1">
                    {t("settings.minimizeToTrayDescription")}
                  </p>
                </div>
                <input
                  type="checkbox"
                  checked={settings.minimizeToTrayOnClose}
                  onChange={(e) =>
                    setSettings((prev) => ({
                      ...prev,
                      minimizeToTrayOnClose: e.target.checked,
                    }))
                  }
                  className="w-4 h-4 text-blue-500 rounded focus:ring-blue-500/20"
                />
              </label>
              {/* Claude 插件联动开关 */}
              <label className="flex items-center justify-between">
                <div>
                  <span className="text-sm text-gray-900 dark:text-gray-100">
                    {t("settings.enableClaudePluginIntegration")}
                  </span>
                  <p className="text-xs text-gray-500 dark:text-gray-400 mt-1 max-w-[34rem]">
                    {t("settings.enableClaudePluginIntegrationDescription")}
                  </p>
                </div>
                <input
                  type="checkbox"
                  checked={!!settings.enableClaudePluginIntegration}
                  onChange={(e) =>
                    setSettings((prev) => ({
                      ...prev,
                      enableClaudePluginIntegration: e.target.checked,
                    }))
                  }
                  className="w-4 h-4 text-blue-500 rounded focus:ring-blue-500/20"
                />
              </label>
            </div>
          </div>

          {/* 配置文件位置 */}
          <div>
            <h3 className="text-sm font-medium text-gray-900 dark:text-gray-100 mb-3">
              {t("settings.configFileLocation")}
            </h3>
            <div className="flex items-center gap-2">
              <div className="flex-1 px-3 py-2 bg-gray-100 dark:bg-gray-800 rounded-lg">
                <span className="text-xs font-mono text-gray-500 dark:text-gray-400">
                  {configPath || t("common.loading")}
                </span>
              </div>
              <button
                onClick={handleOpenConfigFolder}
                className="p-2 hover:bg-gray-100 dark:hover:bg-gray-800 rounded-lg transition-colors"
                title={t("settings.openFolder")}
              >
                <FolderOpen
                  size={18}
                  className="text-gray-500 dark:text-gray-400"
                />
              </button>
            </div>
          </div>

          {/* 配置目录覆盖 */}
          <div>
            <h3 className="text-sm font-medium text-gray-900 dark:text-gray-100 mb-2">
              {t("settings.configDirectoryOverride")}
            </h3>
            <p className="text-xs text-gray-500 dark:text-gray-400 mb-3 leading-relaxed">
              {t("settings.configDirectoryDescription")}
            </p>
            <div className="space-y-3">
              <div>
                <label className="block text-xs font-medium text-gray-500 dark:text-gray-400 mb-1">
                  {t("settings.appConfigDir")}
                </label>
                <p className="text-xs text-gray-400 dark:text-gray-500 mb-1">
                  {t("settings.appConfigDirDescription")}
                </p>
                <div className="flex gap-2">
                  <input
                    type="text"
                    value={appConfigDir ?? resolvedAppConfigDir ?? ""}
                    onChange={(e) => setAppConfigDir(e.target.value)}
                    placeholder={t("settings.browsePlaceholderApp")}
                    className="flex-1 px-3 py-2 text-xs font-mono bg-white dark:bg-gray-800 border border-gray-200 dark:border-gray-700 rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500/40"
                  />
                  <button
                    type="button"
                    onClick={handleBrowseAppConfigDir}
                    className="px-2 py-2 text-xs text-gray-500 dark:text-gray-400 hover:text-blue-500 dark:hover:text-blue-400 hover:bg-gray-100 dark:hover:bg-gray-800 rounded-lg transition-colors"
                    title={t("settings.browseDirectory")}
                  >
                    <FolderSearch size={16} />
                  </button>
                  <button
                    type="button"
                    onClick={handleResetAppConfigDir}
                    className="px-2 py-2 text-xs text-gray-500 dark:text-gray-400 hover:text-blue-500 dark:hover:text-blue-400 hover:bg-gray-100 dark:hover:bg-gray-800 rounded-lg transition-colors"
                    title={t("settings.resetDefault")}
                  >
                    <Undo2 size={16} />
                  </button>
                </div>
              </div>

              <div>
                <label className="block text-xs font-medium text-gray-500 dark:text-gray-400 mb-1">
                  {t("settings.claudeConfigDir")}
                </label>
                <div className="flex gap-2">
                  <input
                    type="text"
                    value={settings.claudeConfigDir ?? resolvedClaudeDir ?? ""}
                    onChange={(e) =>
                      setSettings({
                        ...settings,
                        claudeConfigDir: e.target.value,
                      })
                    }
                    placeholder={t("settings.browsePlaceholderClaude")}
                    className="flex-1 px-3 py-2 text-xs font-mono bg-white dark:bg-gray-800 border border-gray-200 dark:border-gray-700 rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500/40"
                  />
                  <button
                    type="button"
                    onClick={() => handleBrowseConfigDir("claude")}
                    className="px-2 py-2 text-xs text-gray-500 dark:text-gray-400 hover:text-blue-500 dark:hover:text-blue-400 hover:bg-gray-100 dark:hover:bg-gray-800 rounded-lg transition-colors"
                    title={t("settings.browseDirectory")}
                  >
                    <FolderSearch size={16} />
                  </button>
                  <button
                    type="button"
                    onClick={() => handleResetConfigDir("claude")}
                    className="px-2 py-2 text-xs text-gray-500 dark:text-gray-400 hover:text-blue-500 dark:hover:text-blue-400 hover:bg-gray-100 dark:hover:bg-gray-800 rounded-lg transition-colors"
                    title={t("settings.resetDefault")}
                  >
                    <Undo2 size={16} />
                  </button>
                </div>
              </div>

              <div>
                <label className="block text-xs font-medium text-gray-500 dark:text-gray-400 mb-1">
                  {t("settings.codexConfigDir")}
                </label>
                <div className="flex gap-2">
                  <input
                    type="text"
                    value={settings.codexConfigDir ?? resolvedCodexDir ?? ""}
                    onChange={(e) =>
                      setSettings({
                        ...settings,
                        codexConfigDir: e.target.value,
                      })
                    }
                    placeholder={t("settings.browsePlaceholderCodex")}
                    className="flex-1 px-3 py-2 text-xs font-mono bg-white dark:bg-gray-800 border border-gray-200 dark:border-gray-700 rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500/40"
                  />
                  <button
                    type="button"
                    onClick={() => handleBrowseConfigDir("codex")}
                    className="px-2 py-2 text-xs text-gray-500 dark:text-gray-400 hover:text-blue-500 dark:hover:text-blue-400 hover:bg-gray-100 dark:hover:bg-gray-800 rounded-lg transition-colors"
                    title={t("settings.browseDirectory")}
                  >
                    <FolderSearch size={16} />
                  </button>
                  <button
                    type="button"
                    onClick={() => handleResetConfigDir("codex")}
                    className="px-2 py-2 text-xs text-gray-500 dark:text-gray-400 hover:text-blue-500 dark:hover:text-blue-400 hover:bg-gray-100 dark:hover:bg-gray-800 rounded-lg transition-colors"
                    title={t("settings.resetDefault")}
                  >
                    <Undo2 size={16} />
                  </button>
                </div>
              </div>
            </div>
          </div>

          {/* 导入导出 */}
          <div>
            <h3 className="text-sm font-medium text-gray-900 dark:text-gray-100 mb-3">
              {t("settings.importExport")}
            </h3>
            <div className="p-4 bg-gray-100 dark:bg-gray-800 rounded-lg">
              <div className="space-y-3">
                {/* 导出按钮 */}
                <button
                  onClick={handleExportConfig}
                  className="w-full flex items-center justify-center gap-2 px-3 py-2 text-xs font-medium rounded-lg transition-colors bg-gray-500 hover:bg-gray-600 dark:bg-gray-600 dark:hover:bg-gray-700 text-white"
                >
                  <Save size={12} />
                  {t("settings.exportConfig")}
                </button>

                {/* 导入区域 */}
                <div className="space-y-2">
                  <div className="flex items-center gap-2">
                    <button
                      onClick={handleSelectImportFile}
                      className="flex-1 flex items-center justify-center gap-2 px-3 py-2 text-xs font-medium rounded-lg transition-colors bg-gray-500 hover:bg-gray-600 dark:bg-gray-600 dark:hover:bg-gray-700 text-white"
                    >
                      <FolderOpen size={12} />
                      {t("settings.selectConfigFile")}
                    </button>
                    <button
                      onClick={handleExecuteImport}
                      disabled={!selectedImportFile || isImporting}
                      className={`px-3 py-2 text-xs font-medium rounded-lg transition-colors text-white ${
                        !selectedImportFile || isImporting
                          ? "bg-gray-400 cursor-not-allowed"
                          : "bg-blue-500 hover:bg-blue-600 dark:bg-blue-600 dark:hover:bg-blue-700"
                      }`}
                    >
                      {isImporting
                        ? t("settings.importing")
                        : t("settings.import")}
                    </button>
                  </div>

                  {/* 显示选择的文件 */}
                  {selectedImportFile && (
                    <div className="text-xs text-gray-600 dark:text-gray-400 px-2 py-1 bg-gray-50 dark:bg-gray-900 rounded break-all">
                      {selectedImportFile.split("/").pop() ||
                        selectedImportFile.split("\\").pop() ||
                        selectedImportFile}
                    </div>
                  )}
                </div>
              </div>
            </div>
          </div>

          {/* 关于 */}
          <div>
            <h3 className="text-sm font-medium text-gray-900 dark:text-gray-100 mb-3">
              {t("common.about")}
            </h3>
            <div className="p-4 bg-gray-100 dark:bg-gray-800 rounded-lg">
              <div className="flex items-start justify-between">
                <div>
                  <div className="text-sm">
                    <p className="font-medium text-gray-900 dark:text-gray-100">
                      CC Switch
                    </p>
                    <p className="mt-1 text-gray-500 dark:text-gray-400">
                      {t("common.version")} {version}
                    </p>
                  </div>
                </div>
                <div className="flex items-center gap-2">
                  <button
                    onClick={handleOpenReleaseNotes}
                    className="px-2 py-1 text-xs font-medium text-blue-500 hover:text-blue-600 dark:text-blue-400 dark:hover:text-blue-300 rounded-lg hover:bg-blue-500/10 transition-colors"
                    title={
                      hasUpdate
                        ? t("settings.viewReleaseNotes")
                        : t("settings.viewCurrentReleaseNotes")
                    }
                  >
                    <span className="inline-flex items-center gap-1">
                      <ExternalLink size={12} />
                      {t("settings.releaseNotes")}
                    </span>
                  </button>
                  <button
                    onClick={handleCheckUpdate}
                    disabled={isCheckingUpdate || isDownloading}
                    className={`min-w-[88px] px-3 py-1.5 text-xs font-medium rounded-lg transition-all ${
                      isCheckingUpdate || isDownloading
                        ? "bg-gray-100 dark:bg-gray-700 text-gray-400 dark:text-gray-500 cursor-not-allowed border border-transparent"
                        : hasUpdate
                          ? "bg-blue-500 hover:bg-blue-600 dark:bg-blue-600 dark:hover:bg-blue-700 text-white border border-transparent"
                          : showUpToDate
                            ? "bg-green-50 dark:bg-green-900/20 text-green-600 dark:text-green-400 border border-green-200 dark:border-green-800"
                            : "bg-white dark:bg-gray-700 hover:bg-gray-50 dark:hover:bg-gray-600 text-blue-500 dark:text-blue-400 border border-gray-200 dark:border-gray-600"
                    }`}
                  >
                    {isDownloading ? (
                      <span className="flex items-center gap-1">
                        <Download size={12} className="animate-pulse" />
                        {t("settings.updating")}
                      </span>
                    ) : isCheckingUpdate ? (
                      <span className="flex items-center gap-1">
                        <RefreshCw size={12} className="animate-spin" />
                        {t("settings.checking")}
                      </span>
                    ) : hasUpdate ? (
                      <span className="flex items-center gap-1">
                        <Download size={12} />
                        {t("settings.updateTo", {
                          version: updateInfo?.availableVersion ?? "",
                        })}
                      </span>
                    ) : showUpToDate ? (
                      <span className="flex items-center gap-1">
                        <Check size={12} />
                        {t("settings.upToDate")}
                      </span>
                    ) : (
                      t("settings.checkForUpdates")
                    )}
                  </button>
                </div>
              </div>
            </div>
          </div>
        </div>

        {/* 底部按钮 */}
        <div className="flex justify-end gap-3 px-6 py-4 border-t border-gray-200 dark:border-gray-800">
          <button
            onClick={handleCancel}
            className="px-4 py-2 text-sm font-medium text-gray-500 dark:text-gray-400 hover:bg-gray-100 dark:hover:bg-gray-800 rounded-lg transition-colors"
          >
            {t("common.cancel")}
          </button>
          <button
            onClick={saveSettings}
            className="px-4 py-2 text-sm font-medium text-white bg-blue-500 hover:bg-blue-600 dark:bg-blue-600 dark:hover:bg-blue-700 rounded-lg transition-colors flex items-center gap-2"
          >
            <Save size={16} />
            {t("common.save")}
          </button>
        </div>
      </div>

      {/* Import Progress Modal */}
      {importStatus !== "idle" && (
        <ImportProgressModal
          status={importStatus}
          message={importError}
          backupId={importBackupId}
          onComplete={() => {
            setImportStatus("idle");
            setImportError("");
            setSelectedImportFile("");
          }}
          onSuccess={() => {
            if (onImportSuccess) {
              void onImportSuccess();
            }
            void window.api
              .updateTrayMenu()
              .catch((error) =>
                console.error(
                  "[SettingsModal] Failed to refresh tray menu",
                  error,
                ),
              );
          }}
        />
      )}

      {/* Restart Confirmation Dialog */}
      {showRestartDialog && (
        <div className="fixed inset-0 z-[60] flex items-center justify-center">
          <div
            className={`absolute inset-0 bg-black/50 dark:bg-black/70${
              isLinux() ? "" : " backdrop-blur-sm"
            }`}
          />
          <div className="relative bg-white dark:bg-gray-900 rounded-xl shadow-2xl w-[400px] p-6">
            <h3 className="text-lg font-semibold text-gray-900 dark:text-gray-100 mb-3">
              {t("settings.restartRequired")}
            </h3>
            <p className="text-sm text-gray-600 dark:text-gray-400 mb-6">
              {t("settings.restartRequiredMessage")}
            </p>
            <div className="flex justify-end gap-3">
              <button
                onClick={handleRestartLater}
                className="px-4 py-2 text-sm font-medium text-gray-500 dark:text-gray-400 hover:bg-gray-100 dark:hover:bg-gray-800 rounded-lg transition-colors"
              >
                {t("settings.restartLater")}
              </button>
              <button
                onClick={handleRestartNow}
                className="px-4 py-2 text-sm font-medium text-white bg-blue-500 hover:bg-blue-600 dark:bg-blue-600 dark:hover:bg-blue-700 rounded-lg transition-colors"
              >
                {t("settings.restartNow")}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}