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
import { homeDir, join } from "@tauri-apps/api/path";
import "../lib/tauri-api";
import { relaunchApp } from "../lib/updater";
import { useUpdate } from "../contexts/UpdateContext";
import type { Settings } from "../types";
import type { AppType } from "../lib/tauri-api";
import { isLinux } from "../lib/platform";

interface SettingsModalProps {
  onClose: () => void;
}

export default function SettingsModal({ onClose }: SettingsModalProps) {
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
    claudeConfigDir: undefined,
    codexConfigDir: undefined,
    language: persistedLanguage,
  });
  const [initialLanguage, setInitialLanguage] = useState<"zh" | "en">(
    persistedLanguage,
  );
  const [configPath, setConfigPath] = useState<string>("");
  const [version, setVersion] = useState<string>("");
  const [isCheckingUpdate, setIsCheckingUpdate] = useState(false);
  const [isDownloading, setIsDownloading] = useState(false);
  const [showUpToDate, setShowUpToDate] = useState(false);
  const [resolvedClaudeDir, setResolvedClaudeDir] = useState<string>("");
  const [resolvedCodexDir, setResolvedCodexDir] = useState<string>("");
  const [isPortable, setIsPortable] = useState(false);
  const { hasUpdate, updateInfo, updateHandle, checkUpdate, resetDismiss } =
    useUpdate();

  useEffect(() => {
    loadSettings();
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
      await window.api.saveSettings(payload);
      setSettings(payload);
      try {
        window.localStorage.setItem("language", selectedLanguage);
      } catch (error) {
        console.warn("[Settings] Failed to persist language preference", error);
      }
      setInitialLanguage(selectedLanguage);
      if (i18n.language !== selectedLanguage) {
        void i18n.changeLanguage(selectedLanguage);
      }
      onClose();
    } catch (error) {
      console.error(t("console.saveSettingsFailed"), error);
    }
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
            </div>
          </div>

          {/* VS Code 自动同步设置已移除 */}

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
    </div>
  );
}
