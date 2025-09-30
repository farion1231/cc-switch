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
  Cloud,
  CloudUpload,
  CloudDownload,
  Shield,
  Key,

} from "lucide-react";
import { getVersion } from "@tauri-apps/api/app";
import { ImportProgressModal } from "./ImportProgressModal";
import { homeDir, join } from "@tauri-apps/api/path";
import { type AppType } from "../lib/tauri-api";
import { relaunchApp } from "../lib/updater";
import { useUpdate } from "../contexts/UpdateContext";
import type { Settings } from "../types";
import { isLinux } from "../lib/platform";

interface SettingsModalProps {
  onClose: () => void;
  onImportSuccess?: () => void;  // 新增导入成功回调
}

export default function SettingsModal({ onClose, onImportSuccess }: SettingsModalProps) {
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

  // 云同步相关状态
  const [cloudSyncConfig, setCloudSyncConfig] = useState({
    githubToken: "",
    encryptionPassword: "",
    gistUrl: "",
    configured: false,
    enabled: false,
  });
  const [isValidatingToken, setIsValidatingToken] = useState(false);
  const [isSyncing, setIsSyncing] = useState(false);
  const [tokenValid, setTokenValid] = useState<boolean | null>(null);
  const [isImporting, setIsImporting] = useState(false);
  const [importStatus, setImportStatus] = useState<'idle' | 'importing' | 'success' | 'error'>('idle');
  const [importError, setImportError] = useState<string>("");
  const [importBackupId, setImportBackupId] = useState<string>("");
  const [selectedImportFile, setSelectedImportFile] = useState<string>(''); // 新增：保存选择的文件路径

  useEffect(() => {
    loadSettings();
    loadConfigPath();
    loadVersion();
    loadResolvedDirs();
    loadPortableFlag();
    loadCloudSyncSettings();
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

  const loadCloudSyncSettings = async () => {
    try {
      // getSettings 不需要密码参数，因为我们只是获取非敏感信息
      const settings = await window.api.cloudSync.getSettings("");

      // 只更新非敏感信息
      setCloudSyncConfig(prev => ({
        ...prev,
        gistUrl: settings.gistUrl || "",
        configured: settings.configured || false,
        enabled: settings.enabled || false,
      }));

      // 如果后端有 token，说明已经配置过
      if (settings.hasToken) {
        setTokenValid(true);
      }
    } catch (error) {
      console.error("加载云同步设置失败:", error);
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
          "https://github.com/farion1231/cc-switch/releases"
        );
        return;
      }
      const tag = targetVersion.startsWith("v")
        ? targetVersion
        : `v${targetVersion}`;
      await window.api.openExternal(
        `https://github.com/farion1231/cc-switch/releases/tag/${tag}`
      );
    } catch (error) {
      console.error(t("console.openReleaseNotesFailed"), error);
    }
  };

  // 云同步相关函数
  const handleValidateToken = async () => {
    if (!cloudSyncConfig.githubToken.trim()) {
      setTokenValid(false);
      return;
    }

    setIsValidatingToken(true);
    try {
      const result = await window.api.cloudSync.validateGitHubToken(
        cloudSyncConfig.githubToken.trim()
      );
      setTokenValid(result.valid);
    } catch (error) {
      console.error("验证 Token 失败:", error);
      setTokenValid(false);
    } finally {
      setIsValidatingToken(false);
    }
  };

  const handleConfigureCloudSync = async () => {
    // 如果是重新配置，需要新的 token
    const hasNewToken = cloudSyncConfig.githubToken.trim() && cloudSyncConfig.githubToken !== "[已配置]";

    // 如果是新配置，或者提供了新 token，需要验证
    if (!cloudSyncConfig.configured || hasNewToken) {
      if (!tokenValid || !hasNewToken || !cloudSyncConfig.encryptionPassword.trim()) {
        alert("请先输入并验证 GitHub Token，以及设置加密密码");
        return;
      }
    }

    try {
      const result = await window.api.cloudSync.configure({
        githubToken: hasNewToken ? cloudSyncConfig.githubToken.trim() : "",  // 如果没有新 token，发送空字符串
        gistUrl: cloudSyncConfig.gistUrl.trim() || undefined,
        encryptionPassword: cloudSyncConfig.encryptionPassword.trim(),
        autoSyncEnabled: false,
        syncOnStartup: false
      });

      if (result.success) {
        setCloudSyncConfig(prev => ({
          ...prev,
          configured: true,
          enabled: true,
          gistUrl: prev.gistUrl // 保持现有的 Gist URL
        }));

        alert("云同步配置成功！");
      }
    } catch (error) {
      console.error("配置云同步失败:", error);
      alert(`配置云同步失败: ${error}`);
    }
  };

  const handleSyncToCloud = async () => {
    if (!cloudSyncConfig.configured && !cloudSyncConfig.encryptionPassword.trim()) {
      alert("请先配置云同步并输入加密密码");
      return;
    }

    if (!cloudSyncConfig.encryptionPassword.trim()) {
      alert("请输入加密密码");
      return;
    }

    setIsSyncing(true);
    try {
      const result = await window.api.cloudSync.syncToCloud(
        cloudSyncConfig.encryptionPassword
      );

      if (result.success && result.gistUrl) {
        setCloudSyncConfig(prev => ({
          ...prev,
          gistUrl: result.gistUrl,
          configured: true,
          enabled: true
        }));
        // 显示成功消息，包含可复制的 Gist URL
        const message = `✅ 配置已成功同步到云端！\n\n📋 Gist URL 已保存并自动填充到输入框\n${result.gistUrl}\n\n您可以通过此链接在 GitHub 上查看加密的配置。`;
        alert(message);

        // 自动重新加载云同步设置以确保 UI 更新
        await loadCloudSyncSettings();
      }
    } catch (error) {
      console.error("同步到云端失败:", error);
      alert(`同步失败: ${error}`);
    } finally {
      setIsSyncing(false);
    }
  };

  const handleSyncFromCloud = async () => {
    if (!cloudSyncConfig.gistUrl.trim()) {
      alert("请输入 Gist URL");
      return;
    }

    if (!cloudSyncConfig.encryptionPassword.trim()) {
      alert("请输入加密密码");
      return;
    }

    setIsSyncing(true);
    try {
      await window.api.cloudSync.syncFromCloud(
        cloudSyncConfig.gistUrl,
        cloudSyncConfig.encryptionPassword,
        true // auto apply
      );
      alert("配置已成功从云端同步并应用！");

      // 刷新数据
      if (onImportSuccess) {
        onImportSuccess();
      }

      // 关闭设置页面
      onClose();
    } catch (error) {
      console.error("从云端同步失败:", error);
      alert(`同步失败: ${error}`);
    } finally {
      setIsSyncing(false);
    }
  };

  // 导出配置到文件
  const handleExportConfig = async () => {
    try {
      // 使用 Tauri 的保存文件对话框
      const defaultName = `cc-switch-config-${new Date().toISOString().split('T')[0]}.json`;
      const filePath = await window.api.saveFileDialog(defaultName);

      if (!filePath) return; // 用户取消了

      const result = await window.api.exportConfigToFile(filePath);

      if (result.success) {
        alert(`配置已导出到：\n${result.filePath}`);
      }
    } catch (error) {
      console.error("导出配置失败:", error);
      alert(`导出失败: ${error}`);
    }
  };

  // 选择要导入的文件
  const handleSelectImportFile = async () => {
    try {
      const filePath = await window.api.openFileDialog();
      if (filePath) {
        setSelectedImportFile(filePath);
        setImportStatus('idle'); // 重置状态
        setImportError('');
      }
    } catch (error) {
      console.error('选择文件失败:', error);
      alert(`选择文件失败: ${error}`);
    }
  };

  // 执行导入
  const handleExecuteImport = async () => {
    if (!selectedImportFile || isImporting) return;

    setIsImporting(true);
    setImportStatus('importing');

    try {
      const result = await window.api.importConfigFromFile(selectedImportFile);

      if (result.success) {
        setImportBackupId(result.backupId || '');
        setImportStatus('success');
        // ImportProgressModal 组件会在2秒后自动重新加载
      } else {
        setImportError(result.message || '配置文件可能已损坏');
        setImportStatus('error');
        setIsImporting(false);
      }
    } catch (error) {
      setImportError(String(error));
      setImportStatus('error');
      setIsImporting(false);
    }
  };

  return (
    <>
      {/* 导入进度模态框 */}
      {importStatus !== 'idle' && (
        <ImportProgressModal
          status={importStatus}
          message={importError}
          backupId={importBackupId}
          onComplete={() => setImportStatus('idle')}
          onSuccess={() => {
            // 导入成功后调用父组件的刷新函数
            if (onImportSuccess) {
              onImportSuccess();
            }
            // 关闭设置页面
            onClose();
          }}
        />
      )}

      {/* 设置模态框 */}
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

          {/* 云同步 */}
          <div>
            <h3 className="text-sm font-medium text-gray-900 dark:text-gray-100 mb-2 flex items-center gap-2">
              <Cloud size={16} />
              云同步
            </h3>
            <p className="text-xs text-gray-500 dark:text-gray-400 mb-4 leading-relaxed">
              通过 GitHub Gist 安全同步配置。配置会被加密后存储在云端，可以在多台设备间同步。
            </p>

            <div className="space-y-4">
              {/* GitHub Token */}
              <div>
                <label className="block text-xs font-medium text-gray-500 dark:text-gray-400 mb-1">
                  <Key size={12} className="inline mr-1" />
                  GitHub Personal Access Token
                </label>
                <div className="flex gap-2">
                  <input
                    type="password"
                    value={cloudSyncConfig.githubToken}
                    onChange={(e) => {
                      setCloudSyncConfig(prev => ({ ...prev, githubToken: e.target.value }));
                      setTokenValid(null);
                    }}
                    placeholder={cloudSyncConfig.configured ? "输入新 Token 以更新" : "ghp_xxxxxxxxxxxxxxxxxxxx"}
                    className="flex-1 px-3 py-2 text-xs font-mono bg-white dark:bg-gray-800 border border-gray-200 dark:border-gray-700 rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500/40"
                  />
                  <button
                    type="button"
                    onClick={handleValidateToken}
                    disabled={isValidatingToken || !cloudSyncConfig.githubToken.trim()}
                    className={`px-3 py-2 text-xs font-medium rounded-lg transition-colors ${
                      tokenValid === true
                        ? "bg-green-50 dark:bg-green-900/20 text-green-600 dark:text-green-400 border border-green-200 dark:border-green-800"
                        : tokenValid === false
                        ? "bg-red-50 dark:bg-red-900/20 text-red-600 dark:text-red-400 border border-red-200 dark:border-red-800"
                        : "bg-gray-100 dark:bg-gray-700 text-gray-600 dark:text-gray-400 hover:bg-gray-200 dark:hover:bg-gray-600 border border-gray-200 dark:border-gray-600"
                    }`}
                  >
                    {isValidatingToken ? (
                      <RefreshCw size={12} className="animate-spin" />
                    ) : tokenValid === true ? (
                      <Check size={12} />
                    ) : tokenValid === false ? (
                      "无效"
                    ) : (
                      "验证"
                    )}
                  </button>
                </div>
                <p className="text-xs text-gray-400 dark:text-gray-500 mt-1">
                  需要创建具有 Gist 权限的 GitHub Token
                </p>
              </div>

              {/* 加密密码 */}
              <div>
                <label className="block text-xs font-medium text-gray-500 dark:text-gray-400 mb-1">
                  <Shield size={12} className="inline mr-1" />
                  加密密码
                </label>
                <input
                  type="password"
                  value={cloudSyncConfig.encryptionPassword}
                  onChange={(e) =>
                    setCloudSyncConfig(prev => ({ ...prev, encryptionPassword: e.target.value }))
                  }
                  placeholder={cloudSyncConfig.configured ? "输入密码以同步" : "用于加密配置的安全密码"}
                  className="w-full px-3 py-2 text-xs bg-white dark:bg-gray-800 border border-gray-200 dark:border-gray-700 rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500/40"
                />
              </div>

              {/* Gist URL（可选） */}
              <div>
                <label className="block text-xs font-medium text-gray-500 dark:text-gray-400 mb-1">
                  Gist URL（可选）
                </label>
                <div className="flex gap-2">
                  <input
                    type="text"
                    value={cloudSyncConfig.gistUrl}
                    onChange={(e) =>
                      setCloudSyncConfig(prev => ({ ...prev, gistUrl: e.target.value }))
                    }
                    placeholder="留空将自动创建新 Gist"
                    className="flex-1 px-3 py-2 text-xs font-mono bg-white dark:bg-gray-800 border border-gray-200 dark:border-gray-700 rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500/40"
                  />
                  {cloudSyncConfig.gistUrl && (
                    <button
                      type="button"
                      onClick={() => cloudSyncConfig.gistUrl && window.api.openExternal(cloudSyncConfig.gistUrl)}
                      className="px-3 py-2 text-xs font-medium bg-gray-100 dark:bg-gray-700 text-gray-600 dark:text-gray-400 hover:bg-gray-200 dark:hover:bg-gray-600 border border-gray-200 dark:border-gray-600 rounded-lg transition-colors flex items-center gap-1"
                    >
                      <ExternalLink size={12} />
                      查看
                    </button>
                  )}
                </div>
                {cloudSyncConfig.gistUrl && (
                  <p className="text-xs text-green-600 dark:text-green-400 mt-1">
                    ✅ Gist URL 已保存，您的配置将同步到此位置
                  </p>
                )}
              </div>

              {/* 操作按钮 */}
              <div className="flex gap-2">
                <button
                  onClick={handleConfigureCloudSync}
                  disabled={!tokenValid || !cloudSyncConfig.githubToken.trim() || !cloudSyncConfig.encryptionPassword.trim()}
                  className={`flex-1 flex items-center justify-center gap-2 px-3 py-2 text-xs font-medium rounded-lg transition-colors ${
                    tokenValid && cloudSyncConfig.githubToken.trim() && cloudSyncConfig.encryptionPassword.trim()
                      ? "bg-blue-500 hover:bg-blue-600 dark:bg-blue-600 dark:hover:bg-blue-700 text-white"
                      : "bg-gray-100 dark:bg-gray-700 text-gray-400 dark:text-gray-500 cursor-not-allowed"
                  }`}
                >
                  <Cloud size={12} />
                  配置云同步
                </button>
                <button
                  onClick={handleSyncToCloud}
                  disabled={!cloudSyncConfig.configured || isSyncing}
                  className={`flex-1 flex items-center justify-center gap-2 px-3 py-2 text-xs font-medium rounded-lg transition-colors ${
                    cloudSyncConfig.configured && !isSyncing
                      ? "bg-green-500 hover:bg-green-600 dark:bg-green-600 dark:hover:bg-green-700 text-white"
                      : "bg-gray-100 dark:bg-gray-700 text-gray-400 dark:text-gray-500 cursor-not-allowed"
                  }`}
                >
                  {isSyncing ? (
                    <RefreshCw size={12} className="animate-spin" />
                  ) : (
                    <CloudUpload size={12} />
                  )}
                  上传配置
                </button>
                <button
                  onClick={handleSyncFromCloud}
                  disabled={!cloudSyncConfig.gistUrl.trim() || isSyncing}
                  className={`flex-1 flex items-center justify-center gap-2 px-3 py-2 text-xs font-medium rounded-lg transition-colors ${
                    cloudSyncConfig.gistUrl.trim() && !isSyncing
                      ? "bg-purple-500 hover:bg-purple-600 dark:bg-purple-600 dark:hover:bg-purple-700 text-white"
                      : "bg-gray-100 dark:bg-gray-700 text-gray-400 dark:text-gray-500 cursor-not-allowed"
                  }`}
                >
                  {isSyncing ? (
                    <RefreshCw size={12} className="animate-spin" />
                  ) : (
                    <CloudDownload size={12} />
                  )}
                  下载配置
                </button>
              </div>

              {/* 本地导入导出 */}
              <div className="flex flex-col gap-3 mt-3 pt-3 border-t border-gray-200 dark:border-gray-700">
                {/* 导出按钮 */}
                <button
                  onClick={handleExportConfig}
                  className="w-full flex items-center justify-center gap-2 px-3 py-2 text-xs font-medium rounded-lg transition-colors bg-gray-500 hover:bg-gray-600 dark:bg-gray-600 dark:hover:bg-gray-700 text-white"
                >
                  <Save size={12} />
                  导出配置到文件
                </button>

                {/* 导入区域 */}
                <div className="space-y-2">
                  <div className="flex items-center gap-2">
                    <button
                      onClick={handleSelectImportFile}
                      className="flex-1 flex items-center justify-center gap-2 px-3 py-2 text-xs font-medium rounded-lg transition-colors bg-gray-500 hover:bg-gray-600 dark:bg-gray-600 dark:hover:bg-gray-700 text-white"
                    >
                      <FolderOpen size={12} />
                      选择配置文件
                    </button>
                    <button
                      onClick={handleExecuteImport}
                      disabled={!selectedImportFile || isImporting}
                      className={`px-3 py-2 text-xs font-medium rounded-lg transition-colors text-white ${
                        !selectedImportFile || isImporting
                          ? 'bg-gray-400 cursor-not-allowed'
                          : 'bg-blue-500 hover:bg-blue-600 dark:bg-blue-600 dark:hover:bg-blue-700'
                      }`}
                    >
                      {isImporting ? '导入中...' : '导入'}
                    </button>
                  </div>

                  {/* 显示选择的文件 */}
                  {selectedImportFile && (
                    <div className="text-xs text-gray-600 dark:text-gray-400 px-2 py-1 bg-gray-50 dark:bg-gray-900 rounded break-all">
                      {selectedImportFile.split('/').pop() || selectedImportFile.split('\\').pop() || selectedImportFile}
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
    </div>
    </>
  );
}
