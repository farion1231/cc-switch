import { useState, useEffect } from "react";
import {
  X,
  RefreshCw,
  FolderOpen,
  Download,
  ExternalLink,
  Check,
  Undo2,
  FolderSearch,
  Cloud,
  CloudUpload,
  CloudDownload,
  Shield,
  Key,
} from "lucide-react";
import { getVersion } from "@tauri-apps/api/app";
import { homeDir, join } from "@tauri-apps/api/path";
import tauriAPI, { type AppType } from "../lib/tauri-api";
import { relaunchApp } from "../lib/updater";
import { useUpdate } from "../contexts/UpdateContext";
import type { Settings } from "../types";
import { isLinux } from "../lib/platform";

interface SettingsModalProps {
  onClose: () => void;
}

export default function SettingsModal({ onClose }: SettingsModalProps) {
  const [settings, setSettings] = useState<Settings>({
    showInTray: true,
    claudeConfigDir: undefined,
    codexConfigDir: undefined,
  });
  const [configPath, setConfigPath] = useState<string>("");
  const [version, setVersion] = useState<string>("");
  const [isCheckingUpdate, setIsCheckingUpdate] = useState(false);
  const [isDownloading, setIsDownloading] = useState(false);
  const [showUpToDate, setShowUpToDate] = useState(false);
  const [resolvedClaudeDir, setResolvedClaudeDir] = useState<string>("");
  const [resolvedCodexDir, setResolvedCodexDir] = useState<string>("");
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

  useEffect(() => {
    loadSettings();
    loadConfigPath();
    loadVersion();
    loadResolvedDirs();
    loadCloudSyncSettings();
  }, []);

  const loadVersion = async () => {
    try {
      const appVersion = await getVersion();
      setVersion(appVersion);
    } catch (error) {
      console.error("获取版本信息失败:", error);
      // 失败时不硬编码版本号，显示为未知
      setVersion("未知");
    }
  };

  const loadSettings = async () => {
    try {
      const loadedSettings = await tauriAPI.getSettings();
      const showInTray =
        (loadedSettings as any)?.showInTray ??
        (loadedSettings as any)?.showInDock ??
        true;
      setSettings({
        showInTray,
        claudeConfigDir:
          typeof (loadedSettings as any)?.claudeConfigDir === "string"
            ? (loadedSettings as any).claudeConfigDir
            : undefined,
        codexConfigDir:
          typeof (loadedSettings as any)?.codexConfigDir === "string"
            ? (loadedSettings as any).codexConfigDir
            : undefined,
      });
    } catch (error) {
      console.error("加载设置失败:", error);
    }
  };

  const loadConfigPath = async () => {
    try {
      const path = await tauriAPI.getAppConfigPath();
      if (path) {
        setConfigPath(path);
      }
    } catch (error) {
      console.error("获取配置路径失败:", error);
    }
  };

  const loadResolvedDirs = async () => {
    try {
      const [claudeDir, codexDir] = await Promise.all([
        tauriAPI.getConfigDir("claude"),
        tauriAPI.getConfigDir("codex"),
      ]);
      setResolvedClaudeDir(claudeDir || "");
      setResolvedCodexDir(codexDir || "");
    } catch (error) {
      console.error("获取配置目录失败:", error);
    }
  };

  const loadCloudSyncSettings = async () => {
    try {
      const settings = await tauriAPI.cloudSync.getSettings("");

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
      };
      await tauriAPI.saveSettings(payload);
      setSettings(payload);
      onClose();
    } catch (error) {
      console.error("保存设置失败:", error);
    }
  };

  const handleCheckUpdate = async () => {
    if (hasUpdate && updateHandle) {
      // 已检测到更新：直接复用 updateHandle 下载并安装，避免重复检查
      setIsDownloading(true);
      try {
        resetDismiss();
        await updateHandle.downloadAndInstall();
        await relaunchApp();
      } catch (error) {
        console.error("更新失败:", error);
        // 更新失败时回退到打开 Releases 页面
        await tauriAPI.checkForUpdates();
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
        console.error("检查更新失败:", error);
        // 在开发模式下，模拟已是最新版本的响应
        if (import.meta.env.DEV) {
          setShowUpToDate(true);
          setTimeout(() => {
            setShowUpToDate(false);
          }, 3000);
        } else {
          // 生产环境下如果更新插件不可用，回退到打开 Releases 页面
          await tauriAPI.checkForUpdates();
        }
      } finally {
        setIsCheckingUpdate(false);
      }
    }
  };

  const handleOpenConfigFolder = async () => {
    try {
      await tauriAPI.openAppConfigFolder();
    } catch (error) {
      console.error("打开配置文件夹失败:", error);
    }
  };

  const handleBrowseConfigDir = async (app: AppType) => {
    try {
      const currentResolved =
        app === "claude"
          ? (settings.claudeConfigDir ?? resolvedClaudeDir)
          : (settings.codexConfigDir ?? resolvedCodexDir);

      const selected = await tauriAPI.selectConfigDirectory(currentResolved);

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
      console.error("选择配置目录失败:", error);
    }
  };

  const computeDefaultConfigDir = async (app: AppType) => {
    try {
      const home = await homeDir();
      const folder = app === "claude" ? ".claude" : ".codex";
      return await join(home, folder);
    } catch (error) {
      console.error("获取默认配置目录失败:", error);
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
      // 如果未知或为空，回退到 releases 首页
      if (!targetVersion || targetVersion === "未知") {
        await tauriAPI.openExternal(
          "https://github.com/farion1231/cc-switch/releases"
        );
        return;
      }
      const tag = targetVersion.startsWith("v")
        ? targetVersion
        : `v${targetVersion}`;
      await tauriAPI.openExternal(
        `https://github.com/farion1231/cc-switch/releases/tag/${tag}`
      );
    } catch (error) {
      console.error("打开更新日志失败:", error);
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
      const result = await tauriAPI.cloudSync.validateGitHubToken(
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
      const result = await tauriAPI.cloudSync.configure({
        githubToken: hasNewToken ? cloudSyncConfig.githubToken.trim() : "",  // 如果没有新 token，发送空字符串
        gistUrl: cloudSyncConfig.gistUrl.trim() || undefined,
        encryptionPassword: cloudSyncConfig.encryptionPassword,
        autoSyncEnabled: true,
        syncOnStartup: false,
      });

      if (result.success) {
        setCloudSyncConfig(prev => ({ ...prev, configured: true, enabled: true }));
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
      const result = await tauriAPI.cloudSync.syncToCloud(
        cloudSyncConfig.encryptionPassword
      );

      if (result.success && result.gistUrl) {
        setCloudSyncConfig(prev => ({ ...prev, gistUrl: result.gistUrl }));
        alert(`配置已成功同步到云端！\nGist URL: ${result.gistUrl}`);
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
      await tauriAPI.cloudSync.syncFromCloud(
        cloudSyncConfig.gistUrl,
        cloudSyncConfig.encryptionPassword,
        true // auto apply
      );
      alert("配置已成功从云端同步并应用！");
    } catch (error) {
      console.error("从云端同步失败:", error);
      alert(`同步失败: ${error}`);
    } finally {
      setIsSyncing(false);
    }
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div
        className={`absolute inset-0 bg-black/50 dark:bg-black/70${
          isLinux() ? "" : " backdrop-blur-sm"
        }`}
      />
      <div className="relative bg-white dark:bg-gray-900 rounded-xl shadow-2xl w-[500px] max-h-[90vh] flex flex-col overflow-hidden">
        {/* 标题栏 */}
        <div className="flex items-center justify-between px-6 py-4 border-b border-gray-200 dark:border-gray-800 flex-shrink-0">
          <h2 className="text-lg font-semibold text-blue-500 dark:text-blue-400">
            设置
          </h2>
          <button
            onClick={onClose}
            className="p-1.5 hover:bg-gray-100 dark:hover:bg-gray-800 rounded-md transition-colors"
          >
            <X size={20} className="text-gray-500 dark:text-gray-400" />
          </button>
        </div>

        {/* 设置内容 - 可滚动区域 */}
        <div className="px-6 py-4 space-y-6 flex-1 overflow-y-auto">
          {/* 系统托盘设置（未实现）
              说明：此开关用于控制是否在系统托盘/菜单栏显示应用图标。 */}
          {/* <div>
            <h3 className="text-sm font-medium text-gray-900 dark:text-gray-100 mb-3">
              显示设置（系统托盘）
            </h3>
            <label className="flex items-center justify-between">
              <span className="text-sm text-gray-500">
                在菜单栏显示图标（系统托盘）
              </span>
              <input
                type="checkbox"
                checked={settings.showInTray}
                onChange={(e) =>
                  setSettings({ ...settings, showInTray: e.target.checked })
                }
                className="w-4 h-4 text-blue-500 rounded focus:ring-blue-500/20"
              />
            </label>
          </div> */}

          {/* VS Code 自动同步设置已移除 */}

          {/* 配置文件位置 */}
          <div>
            <h3 className="text-sm font-medium text-gray-900 dark:text-gray-100 mb-3">
              配置文件位置
            </h3>
            <div className="flex items-center gap-2">
              <div className="flex-1 px-3 py-2 bg-gray-100 dark:bg-gray-800 rounded-lg">
                <span className="text-xs font-mono text-gray-500 dark:text-gray-400">
                  {configPath || "加载中..."}
                </span>
              </div>
              <button
                onClick={handleOpenConfigFolder}
                className="p-2 hover:bg-gray-100 dark:hover:bg-gray-800 rounded-lg transition-colors"
                title="打开文件夹"
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
              配置目录覆盖（高级）
            </h3>
            <p className="text-xs text-gray-500 dark:text-gray-400 mb-3 leading-relaxed">
              在 WSL 等环境使用 Claude Code 或 Codex 的时候，可手动指定 WSL
              里的配置目录，供应商数据与主环境保持一致。
            </p>
            <div className="space-y-3">
              <div>
                <label className="block text-xs font-medium text-gray-500 dark:text-gray-400 mb-1">
                  Claude Code 配置目录
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
                    placeholder="例如：/home/<你的用户名>/.claude"
                    className="flex-1 px-3 py-2 text-xs font-mono bg-white dark:bg-gray-800 border border-gray-200 dark:border-gray-700 rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500/40"
                  />
                  <button
                    type="button"
                    onClick={() => handleBrowseConfigDir("claude")}
                    className="px-2 py-2 text-xs text-gray-500 dark:text-gray-400 hover:text-blue-500 dark:hover:text-blue-400 hover:bg-gray-100 dark:hover:bg-gray-800 rounded-lg transition-colors"
                    title="浏览目录"
                  >
                    <FolderSearch size={16} />
                  </button>
                  <button
                    type="button"
                    onClick={() => handleResetConfigDir("claude")}
                    className="px-2 py-2 text-xs text-gray-500 dark:text-gray-400 hover:text-blue-500 dark:hover:text-blue-400 hover:bg-gray-100 dark:hover:bg-gray-800 rounded-lg transition-colors"
                    title="恢复默认目录（需保存后生效）"
                  >
                    <Undo2 size={16} />
                  </button>
                </div>
              </div>

              <div>
                <label className="block text-xs font-medium text-gray-500 dark:text-gray-400 mb-1">
                  Codex 配置目录
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
                    placeholder="例如：/home/<你的用户名>/.codex"
                    className="flex-1 px-3 py-2 text-xs font-mono bg-white dark:bg-gray-800 border border-gray-200 dark:border-gray-700 rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500/40"
                  />
                  <button
                    type="button"
                    onClick={() => handleBrowseConfigDir("codex")}
                    className="px-2 py-2 text-xs text-gray-500 dark:text-gray-400 hover:text-blue-500 dark:hover:text-blue-400 hover:bg-gray-100 dark:hover:bg-gray-800 rounded-lg transition-colors"
                    title="浏览目录"
                  >
                    <FolderSearch size={16} />
                  </button>
                  <button
                    type="button"
                    onClick={() => handleResetConfigDir("codex")}
                    className="px-2 py-2 text-xs text-gray-500 dark:text-gray-400 hover:text-blue-500 dark:hover:text-blue-400 hover:bg-gray-100 dark:hover:bg-gray-800 rounded-lg transition-colors"
                    title="恢复默认目录（需保存后生效）"
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
                <input
                  type="text"
                  value={cloudSyncConfig.gistUrl}
                  onChange={(e) =>
                    setCloudSyncConfig(prev => ({ ...prev, gistUrl: e.target.value }))
                  }
                  placeholder="留空将自动创建新 Gist"
                  className="w-full px-3 py-2 text-xs font-mono bg-white dark:bg-gray-800 border border-gray-200 dark:border-gray-700 rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500/40"
                />
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
            </div>
          </div>

          {/* 关于 */}
          <div>
            <h3 className="text-sm font-medium text-gray-900 dark:text-gray-100 mb-3">
              关于
            </h3>
            <div className="p-4 bg-gray-100 dark:bg-gray-800 rounded-lg">
              <div className="flex items-start justify-between">
                <div>
                  <div className="text-sm">
                    <p className="font-medium text-gray-900 dark:text-gray-100">
                      CC Switch
                    </p>
                    <p className="mt-1 text-gray-500 dark:text-gray-400">
                      版本 {version}
                    </p>
                  </div>
                </div>
                <div className="flex items-center gap-2">
                  <button
                    onClick={handleOpenReleaseNotes}
                    className="px-2 py-1 text-xs font-medium text-blue-500 hover:text-blue-600 dark:text-blue-400 dark:hover:text-blue-300 rounded-lg hover:bg-blue-500/10 transition-colors"
                    title={
                      hasUpdate ? "查看该版本更新日志" : "查看当前版本更新日志"
                    }
                  >
                    <span className="inline-flex items-center gap-1">
                      <ExternalLink size={12} />
                      更新日志
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
                        更新中...
                      </span>
                    ) : isCheckingUpdate ? (
                      <span className="flex items-center gap-1">
                        <RefreshCw size={12} className="animate-spin" />
                        检查中...
                      </span>
                    ) : hasUpdate ? (
                      <span className="flex items-center gap-1">
                        <Download size={12} />
                        更新到 v{updateInfo?.availableVersion}
                      </span>
                    ) : showUpToDate ? (
                      <span className="flex items-center gap-1">
                        <Check size={12} />
                        已是最新
                      </span>
                    ) : (
                      "检查更新"
                    )}
                  </button>
                </div>
              </div>
            </div>
          </div>
        </div>

        {/* 底部按钮 */}
        <div className="flex justify-end gap-3 px-6 py-4 border-t border-gray-200 dark:border-gray-800 flex-shrink-0">
          <button
            onClick={onClose}
            className="px-4 py-2 text-sm font-medium text-gray-500 dark:text-gray-400 hover:bg-gray-100 dark:hover:bg-gray-800 rounded-lg transition-colors"
          >
            取消
          </button>
          <button
            onClick={saveSettings}
            className="px-4 py-2 text-sm font-medium text-white bg-blue-500 hover:bg-blue-600 dark:bg-blue-600 dark:hover:bg-blue-700 rounded-lg transition-colors"
          >
            保存
          </button>
        </div>
      </div>
    </div>
  );
}
