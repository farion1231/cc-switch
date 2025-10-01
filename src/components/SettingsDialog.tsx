import { useState, useEffect } from "react";
import { useTranslation } from "react-i18next";
import {
  RefreshCw,
  FolderOpen,
  Download,
  ExternalLink,
  Check,
  Undo2,
  FolderSearch,
  Save,
} from "lucide-react";
import { relaunchApp } from "../lib/updater";
import { useUpdate } from "../contexts/UpdateContext";
import {
  useSettingsQuery,
  useSaveSettingsMutation,
  useAppConfigPathQuery,
  useConfigDirQuery,
  useIsPortableQuery,
  useVersionQuery,
} from "../lib/query";
import type { Settings } from "../types";
import type { AppType } from "../lib/query";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Checkbox } from "@/components/ui/checkbox";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogClose,
  DialogFooter,
} from "@/components/ui/dialog";

interface SettingsDialogProps {
  onOpenChange: (open: boolean) => void;
}

export function SettingsDialog({ onOpenChange }: SettingsDialogProps) {
  const { t, i18n } = useTranslation();
  const { hasUpdate, updateInfo, updateHandle, checkUpdate, resetDismiss } =
    useUpdate();

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

  // State hooks (must be called before any conditional returns)
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
  const [isCheckingUpdate, setIsCheckingUpdate] = useState(false);
  const [isDownloading, setIsDownloading] = useState(false);
  const [showUpToDate, setShowUpToDate] = useState(false);

  // TanStack Query hooks
  const { data: settingsData, isLoading: isLoadingSettings } = useSettingsQuery();
  const saveSettingsMutation = useSaveSettingsMutation();
  const { data: configPath, isLoading: isLoadingConfigPath } = useAppConfigPathQuery();
  const { data: claudeConfigDir } = useConfigDirQuery("claude");
  const { data: codexConfigDir } = useConfigDirQuery("codex");
  const { data: isPortable } = useIsPortableQuery();
  const { data: version, isLoading: isLoadingVersion } = useVersionQuery();

  const isLoading = isLoadingSettings || isLoadingConfigPath || isLoadingVersion;

  // Initialize settings when data is loaded
  useEffect(() => {
    if (settingsData) {
      const loadedSettings = settingsData as any;
      const showInTray =
        loadedSettings?.showInTray ??
        loadedSettings?.showInDock ??
        true;
      const minimizeToTrayOnClose =
        loadedSettings?.minimizeToTrayOnClose ??
        loadedSettings?.minimize_to_tray_on_close ??
        true;
      const storedLanguage = normalizeLanguage(
        typeof loadedSettings?.language === "string"
          ? loadedSettings.language
          : persistedLanguage,
      );

      setSettings({
        showInTray,
        minimizeToTrayOnClose,
        claudeConfigDir:
          typeof loadedSettings?.claudeConfigDir === "string"
            ? loadedSettings.claudeConfigDir
            : undefined,
        codexConfigDir:
          typeof loadedSettings?.codexConfigDir === "string"
            ? loadedSettings.codexConfigDir
            : undefined,
        language: storedLanguage,
      });
      setInitialLanguage(storedLanguage);
      if (i18n.language !== storedLanguage) {
        void i18n.changeLanguage(storedLanguage);
      }
    }
  }, [settingsData, persistedLanguage, i18n]);


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

      await saveSettingsMutation.mutateAsync(payload);

      try {
        window.localStorage.setItem("language", selectedLanguage);
      } catch (error) {
        console.warn("[Settings] Failed to persist language preference", error);
      }
      setInitialLanguage(selectedLanguage);
      if (i18n.language !== selectedLanguage) {
        void i18n.changeLanguage(selectedLanguage);
      }
      onOpenChange(false);
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
          ? (settings.claudeConfigDir ?? claudeConfigDir ?? "")
          : (settings.codexConfigDir ?? codexConfigDir ?? "");

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
      } else {
        setSettings((prev) => ({ ...prev, codexConfigDir: sanitized }));
      }
    } catch (error) {
      console.error(t("console.selectConfigDirFailed"), error);
    }
  };

  const handleResetConfigDir = async (app: AppType) => {
    setSettings((prev) => ({
      ...prev,
      ...(app === "claude"
        ? { claudeConfigDir: undefined }
        : { codexConfigDir: undefined }),
    }));
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

  return (
    <DialogContent className="w-[500px] max-h-[90vh] overflow-hidden">
        <DialogHeader>
          <DialogTitle className="text-lg font-semibold text-blue-500 dark:text-blue-400">
            {t("settings.title")}
          </DialogTitle>
        </DialogHeader>

        {isLoading ? (
          <div className="text-center py-8">
            <div className="animate-spin rounded-full h-8 w-8 border-b-2 border-blue-500 mx-auto mb-4"></div>
            <p className="text-gray-500 dark:text-gray-400">{t("common.loading")}</p>
          </div>
        ) : (
          <div className="space-y-6 overflow-y-auto flex-1 py-4">
            {/* 语言设置 */}
            <div>
              <Label className="text-sm font-medium mb-3">
                {t("settings.language")}
              </Label>
              <Select
                value={settings.language ?? "zh"}
                onValueChange={(value: "zh" | "en") => handleLanguageChange(value)}
              >
                <SelectTrigger className="w-[200px]">
                  <SelectValue placeholder={t("settings.selectLanguage")} />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="zh">
                    {t("settings.languageOptionChinese")}
                  </SelectItem>
                  <SelectItem value="en">
                    {t("settings.languageOptionEnglish")}
                  </SelectItem>
                </SelectContent>
              </Select>
            </div>

            {/* 窗口行为设置 */}
            <div>
              <Label className="text-sm font-medium mb-3">
                {t("settings.windowBehavior")}
              </Label>
              <div className="space-y-3">
                <div className="flex items-center justify-between space-x-2">
                  <div className="space-y-0.5">
                    <Label className="text-sm font-normal">
                      {t("settings.minimizeToTray")}
                    </Label>
                    <p className="text-xs text-muted-foreground">
                      {t("settings.minimizeToTrayDescription")}
                    </p>
                  </div>
                  <Checkbox
                    checked={settings.minimizeToTrayOnClose}
                    onCheckedChange={(checked) =>
                      setSettings((prev) => ({
                        ...prev,
                        minimizeToTrayOnClose: checked as boolean,
                      }))
                    }
                  />
                </div>
              </div>
            </div>

            {/* 配置文件位置 */}
            <div>
              <Label className="text-sm font-medium mb-3">
                {t("settings.configFileLocation")}
              </Label>
              <div className="flex items-center gap-2">
                <div className="flex-1 px-3 py-2 bg-muted rounded-lg">
                  <span className="text-xs font-mono text-muted-foreground">
                    {configPath || t("common.unknown")}
                  </span>
                </div>
                <Button
                  variant="ghost"
                  size="icon"
                  onClick={handleOpenConfigFolder}
                  title={t("settings.openFolder")}
                >
                  <FolderOpen size={18} />
                </Button>
              </div>
            </div>

            {/* 配置目录覆盖 */}
            <div>
              <Label className="text-sm font-medium mb-2">
                {t("settings.configDirectoryOverride")}
              </Label>
              <p className="text-xs text-muted-foreground mb-3 leading-relaxed">
                {t("settings.configDirectoryDescription")}
              </p>
              <div className="space-y-3">
                <div>
                  <Label className="text-xs font-medium mb-1">
                    {t("settings.claudeConfigDir")}
                  </Label>
                  <div className="flex gap-2">
                    <Input
                      type="text"
                      value={settings.claudeConfigDir ?? claudeConfigDir ?? ""}
                      onChange={(e) =>
                        setSettings({
                          ...settings,
                          claudeConfigDir: e.target.value,
                        })
                      }
                      placeholder={t("settings.browsePlaceholderClaude")}
                      className="flex-1 text-xs font-mono"
                    />
                    <Button
                      type="button"
                      variant="ghost"
                      size="icon"
                      onClick={() => handleBrowseConfigDir("claude")}
                      title={t("settings.browseDirectory")}
                    >
                      <FolderSearch size={16} />
                    </Button>
                    <Button
                      type="button"
                      variant="ghost"
                      size="icon"
                      onClick={() => handleResetConfigDir("claude")}
                      title={t("settings.resetDefault")}
                    >
                      <Undo2 size={16} />
                    </Button>
                  </div>
                </div>

                <div>
                  <Label className="text-xs font-medium mb-1">
                    {t("settings.codexConfigDir")}
                  </Label>
                  <div className="flex gap-2">
                    <Input
                      type="text"
                      value={settings.codexConfigDir ?? codexConfigDir ?? ""}
                      onChange={(e) =>
                        setSettings({
                          ...settings,
                          codexConfigDir: e.target.value,
                        })
                      }
                      placeholder={t("settings.browsePlaceholderCodex")}
                      className="flex-1 text-xs font-mono"
                    />
                    <Button
                      type="button"
                      variant="ghost"
                      size="icon"
                      onClick={() => handleBrowseConfigDir("codex")}
                      title={t("settings.browseDirectory")}
                    >
                      <FolderSearch size={16} />
                    </Button>
                    <Button
                      type="button"
                      variant="ghost"
                      size="icon"
                      onClick={() => handleResetConfigDir("codex")}
                      title={t("settings.resetDefault")}
                    >
                      <Undo2 size={16} />
                    </Button>
                  </div>
                </div>
              </div>
            </div>

            {/* 关于 */}
            <div>
              <Label className="text-sm font-medium mb-3">
                {t("common.about")}
              </Label>
              <div className="p-4 bg-muted rounded-lg">
                <div className="flex items-start justify-between">
                  <div>
                    <div className="text-sm">
                      <p className="font-medium">
                        CC Switch
                      </p>
                      <p className="mt-1 text-muted-foreground">
                        {t("common.version")} {version || t("common.unknown")}
                      </p>
                    </div>
                  </div>
                  <div className="flex items-center gap-2">
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={handleOpenReleaseNotes}
                      title={
                        hasUpdate
                          ? t("settings.viewReleaseNotes")
                          : t("settings.viewCurrentReleaseNotes")
                      }
                    >
                      <ExternalLink size={12} />
                      {t("settings.releaseNotes")}
                    </Button>
                    <Button
                      variant={hasUpdate ? "default" : showUpToDate ? "secondary" : "outline"}
                      size="sm"
                      onClick={handleCheckUpdate}
                      disabled={isCheckingUpdate || isDownloading}
                      className="min-w-[88px]"
                    >
                      {isDownloading ? (
                        <>
                          <Download size={12} className="animate-pulse" />
                          {t("settings.updating")}
                        </>
                      ) : isCheckingUpdate ? (
                        <>
                          <RefreshCw size={12} className="animate-spin" />
                          {t("settings.checking")}
                        </>
                      ) : hasUpdate ? (
                        <>
                          <Download size={12} />
                          {t("settings.updateTo", {
                            version: updateInfo?.availableVersion ?? "",
                          })}
                        </>
                      ) : showUpToDate ? (
                        <>
                          <Check size={12} />
                          {t("settings.upToDate")}
                        </>
                      ) : (
                        t("settings.checkForUpdates")
                      )}
                    </Button>
                  </div>
                </div>
              </div>
            </div>
          </div>
        )}

        {/* 底部按钮 */}
        <DialogFooter className="pt-4">
          <DialogClose asChild>
            <Button variant="outline" onClick={handleCancel}>
              {t("common.cancel")}
            </Button>
          </DialogClose>
          <Button onClick={saveSettings}>
            <Save size={16} />
            {t("common.save")}
          </Button>
        </DialogFooter>
      </DialogContent>
  );
}