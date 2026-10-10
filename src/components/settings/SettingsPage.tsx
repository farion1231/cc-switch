import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type ComponentType,
} from "react";
import {
  Database,
  Folder,
  Globe,
  Info,
  Loader2,
  Route,
  SlidersHorizontal,
} from "lucide-react";
import { toast } from "@/lib/toast";
import { useTranslation } from "react-i18next";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { settingsApi, type AppId } from "@/lib/api";
import { useSettingsQuery } from "@/lib/query";
import type { SettingsSection } from "@/lib/navigation";
import { AppPageHeader } from "@/components/shell/AppPageHeader";
import { DirectoryInput } from "@/components/settings/DirectorySettings";
import { ImportExportSection } from "@/components/settings/ImportExportSection";
import { BackupListSection } from "@/components/settings/BackupListSection";
import { WebdavSyncSection } from "@/components/settings/WebdavSyncSection";
import { AboutSection } from "@/components/settings/AboutSection";
import { RoutingSection } from "@/components/settings/sections/RoutingSection";
import { GlobalProxySettings } from "@/components/settings/GlobalProxySettings";
import { LogConfigPanel } from "@/components/settings/LogConfigPanel";
import { GeneralSection } from "@/components/settings/sections/GeneralSection";
import { AppConfigSection } from "@/components/settings/sections/AppConfigSection";
import {
  SettingsBlock,
  SettingsBody,
  SettingsCard,
  SettingsRow,
} from "@/components/settings/SettingsLayout";
import { useSettings } from "@/hooks/useSettings";
import { useImportExport } from "@/hooks/useImportExport";
import type { SettingsFormState } from "@/hooks/useSettings";
import { isTextEditableTarget } from "@/utils/domUtils";

const SECTION_ICON: Record<
  SettingsSection,
  ComponentType<{ className?: string; strokeWidth?: number | string }>
> = {
  general: SlidersHorizontal,
  appConfig: Folder,
  routing: Route,
  network: Globe,
  data: Database,
  about: Info,
};

interface SettingsPageProps {
  section: SettingsSection;
  /** 从应用页的「配置目录」进入时，定位到该应用的配置区域。 */
  appConfigScrollTarget?: AppId;
  onImportSuccess?: () => void | Promise<void>;
  /** 「在侧栏显示哪些应用」跳到「应用」页 */
  onOpenApps: () => void;
  /** 本地路由 →「正在使用路由的应用」的「前往」 */
  onOpenApp: (app: AppId) => void;
}

/**
 * 设置（v7）：侧栏换成设置目录，这里只渲染当前分组。设置里只放偏好，功能页都在侧栏。
 * 开关类即时保存；路径类改完点本节的「保存」。
 */
export function SettingsPage({
  section,
  appConfigScrollTarget,
  onImportSuccess,
  onOpenApps,
  onOpenApp,
}: SettingsPageProps) {
  const { t } = useTranslation();
  const {
    settings,
    isLoading,
    isSaving,
    isPortable,
    appConfigDir,
    initialAppConfigDir,
    resolvedDirs,
    updateSettings,
    updateDirectory,
    updateAppConfigDir,
    browseDirectory,
    browseAppConfigDir,
    resetDirectory,
    resetAppConfigDir,
    saveSettings,
    autoSaveSettings,
    requiresRestart,
    acknowledgeRestart,
  } = useSettings();
  const { data: savedSettings } = useSettingsQuery();

  const {
    selectedFile,
    status: importStatus,
    errorMessage,
    backupId,
    isImporting,
    selectImportFile,
    importConfig,
    exportConfig,
    clearSelection,
    resetStatus,
  } = useImportExport({ onImportSuccess });

  const [showRestartPrompt, setShowRestartPrompt] = useState(false);
  const scrollRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    resetStatus();
  }, [resetStatus]);

  useEffect(() => {
    if (requiresRestart) {
      setShowRestartPrompt(true);
    }
  }, [requiresRestart]);

  useLayoutEffect(() => {
    if (scrollRef.current) {
      scrollRef.current.scrollTop = 0;
    }
  }, [section]);

  const hasSettings = !!settings;
  useEffect(() => {
    if (section !== "appConfig" || !appConfigScrollTarget || !hasSettings)
      return;

    const container = scrollRef.current;
    if (!container) return;

    let highlight: Animation | undefined;
    let frame = 0;
    const inputListeners = new AbortController();
    const cancelNavigation = () => {
      cancelAnimationFrame(frame);
      highlight?.cancel();
      inputListeners.abort();
    };
    const handleScrollKey = (event: KeyboardEvent) => {
      if (
        !isTextEditableTarget(event.target) &&
        [
          "ArrowUp",
          "ArrowDown",
          "PageUp",
          "PageDown",
          "Home",
          "End",
          " ",
        ].includes(event.key)
      ) {
        cancelNavigation();
      }
    };
    // 用户接管时停止定位，让原生滚动、点击和键盘行为照常执行。
    const inputOptions = {
      capture: true,
      passive: true,
      signal: inputListeners.signal,
    };
    for (const event of ["wheel", "touchstart", "pointerdown"]) {
      container.addEventListener(event, cancelNavigation, inputOptions);
    }
    window.addEventListener("keydown", handleScrollKey, inputOptions);

    // 等条目挂载后，仅滚动设置正文，避免 scrollIntoView 带动外层布局。
    frame = requestAnimationFrame(() => {
      const target = container.querySelector<HTMLElement>(
        `#app-config-${appConfigScrollTarget}`,
      );
      if (!target) {
        inputListeners.abort();
        return;
      }

      const viewportTop =
        container.getBoundingClientRect().top + container.clientTop;
      const viewportBottom = viewportTop + container.clientHeight;
      const targetRect = target.getBoundingClientRect();
      // 为闪烁描边留出 8px，避免底部条目的提示被裁切。
      const topDelta = targetRect.top - viewportTop - 8;
      const bottomDelta = targetRect.bottom - viewportBottom + 8;
      // 只移动到最近的可见位置；已可见（或高度覆盖整个视口）时不滚动。
      const delta =
        topDelta < 0 && bottomDelta < 0
          ? Math.max(topDelta, bottomDelta)
          : topDelta > 0 && bottomDelta > 0
            ? Math.min(topDelta, bottomDelta)
            : 0;
      const startTop = container.scrollTop;
      const endTop = Math.max(
        0,
        Math.min(
          startTop + delta,
          container.scrollHeight - container.clientHeight,
        ),
      );

      // 定位后用两次描边闪烁提示目标，不改变条目尺寸或位置。
      const flashTarget = () => {
        highlight = target.animate?.(
          [
            { boxShadow: "0 0 0 2px transparent" },
            { boxShadow: "0 0 0 2px hsl(var(--ring))" },
            { boxShadow: "0 0 0 2px transparent" },
          ],
          { duration: 650, iterations: 2, easing: "ease-in-out" },
        );
        if (highlight) {
          highlight.onfinish = () => inputListeners.abort();
        } else {
          inputListeners.abort();
        }
      };
      const prefersReducedMotion = window.matchMedia(
        "(prefers-reduced-motion: reduce)",
      ).matches;
      if (startTop === endTop || prefersReducedMotion) {
        container.scrollTop = endTop;
        flashTarget();
        return;
      }

      // 使用同一帧循环完成缓出滚动，结束后再闪烁；切页时可一并取消。
      const startedAt = performance.now();
      let expectedTop = startTop;
      const scrollToTarget = (now: number) => {
        // 原生滚动条可能不派发 pointerdown；位置已被外部改变时同样让出控制。
        if (container.scrollTop !== expectedTop) {
          cancelNavigation();
          return;
        }
        const progress = Math.min((now - startedAt) / 350, 1);
        const eased = 1 - (1 - progress) ** 3;
        container.scrollTop = startTop + (endTop - startTop) * eased;
        expectedTop = container.scrollTop;
        if (progress < 1) {
          frame = requestAnimationFrame(scrollToTarget);
        } else {
          flashTarget();
        }
      };
      frame = requestAnimationFrame(scrollToTarget);
    });

    return cancelNavigation;
  }, [section, appConfigScrollTarget, hasSettings]);

  const afterSave = useCallback(() => {
    acknowledgeRestart();
  }, [acknowledgeRestart]);

  // 路径类设置（配置目录、CC Switch 数据目录）点「保存」才写入；改了数据目录要重启
  const handleSave = useCallback(async () => {
    try {
      const result = await saveSettings(undefined, { silent: false });
      if (!result) return;
      if (result.requiresRestart) {
        setShowRestartPrompt(true);
        return;
      }
      afterSave();
    } catch (error) {
      console.error("[SettingsPage] Failed to save settings", error);
    }
  }, [afterSave, saveSettings]);

  const handleRestartLater = useCallback(() => {
    setShowRestartPrompt(false);
    afterSave();
  }, [afterSave]);

  const handleRestartNow = useCallback(async () => {
    setShowRestartPrompt(false);
    if (import.meta.env.DEV) {
      toast.success(t("settings.devModeRestartHint"), { closeButton: true });
      afterSave();
      return;
    }

    try {
      await settingsApi.restart();
    } catch (error) {
      console.error("[SettingsPage] Failed to restart app", error);
      toast.error(t("settings.restartFailed"));
    } finally {
      afterSave();
    }
  }, [afterSave, t]);

  // 通用设置即时保存（无需手动点击）
  // 使用 autoSaveSettings 避免误触发系统 API（开机自启、Claude 插件等）
  // 返回保存是否成功：需要在保存成功后追加动作的调用方（如统一会话历史
  // 关闭后的备份还原）据此短路，其余调用方可忽略返回值。
  const handleAutoSave = useCallback(
    async (updates: Partial<SettingsFormState>): Promise<boolean> => {
      if (!settings) return false;
      // 乐观更新前捕获旧值：autoSaveSettings 发送的是全量表单状态，后端按
      // diff 触发副作用（如统一会话开关的 live 重写与历史迁移）。保存失败
      // 不回滚的话，失败的变更会滞留在表单里，被之后任意一次无关保存原样
      // 重放，绕过确认弹窗。
      const previousValues = Object.fromEntries(
        Object.keys(updates).map((key) => [
          key,
          settings[key as keyof SettingsFormState],
        ]),
      ) as Partial<SettingsFormState>;
      updateSettings(updates);
      try {
        await autoSaveSettings(updates);
        return true;
      } catch (error) {
        console.error("[SettingsPage] Failed to autosave settings", error);
        updateSettings(previousValues);
        toast.error(
          t("settings.saveFailedGeneric", {
            defaultValue: "保存失败，请重试",
          }),
        );
        return false;
      }
    },
    [autoSaveSettings, settings, t, updateSettings],
  );

  const isBusy = useMemo(() => isLoading && !settings, [isLoading, settings]);
  const SectionIcon = SECTION_ICON[section];

  const appConfigDirty =
    (appConfigDir?.trim() || undefined) !==
    (initialAppConfigDir?.trim() || undefined);

  const renderSection = () => {
    if (!settings) return null;
    switch (section) {
      case "general":
        return (
          <GeneralSection
            settings={settings}
            onAutoSave={handleAutoSave}
            onOpenApps={onOpenApps}
          />
        );
      case "appConfig":
        return (
          <AppConfigSection
            settings={settings}
            savedSettings={savedSettings}
            resolvedDirs={resolvedDirs}
            isSaving={isSaving}
            onAutoSave={handleAutoSave}
            onDirectoryChange={updateDirectory}
            onBrowseDirectory={browseDirectory}
            onResetDirectory={resetDirectory}
            onSaveDirectories={handleSave}
          />
        );
      case "routing":
        return <RoutingSection onOpenApp={onOpenApp} />;
      case "network":
        return (
          <SettingsBlock
            title={t("settings.advanced.globalProxy.title")}
            help={{
              title: t("settings.advanced.globalProxy.title"),
              body: t("settings.advanced.globalProxy.description"),
            }}
          >
            <div className="rounded-panel border border-border bg-surface p-5">
              <GlobalProxySettings />
            </div>
          </SettingsBlock>
        );
      case "data":
        return (
          <>
            <SettingsBlock
              title={t("settings.data.location")}
              actions={
                appConfigDirty ? (
                  <Button
                    variant="solid"
                    size="compact"
                    disabled={isSaving}
                    onClick={() => void handleSave()}
                  >
                    {isSaving && (
                      <Loader2 className="h-3.5 w-3.5 animate-spin" />
                    )}
                    {t("common.save")}
                  </Button>
                ) : null
              }
            >
              <SettingsCard>
                <SettingsRow
                  label={t("settings.appConfigDir")}
                  help={{
                    title: t("settings.appConfigDir"),
                    body: t("settings.appConfigDirDescription"),
                  }}
                >
                  <DirectoryInput
                    label=""
                    value={appConfigDir}
                    resolvedValue={resolvedDirs.appConfig}
                    placeholder={t("settings.browsePlaceholderApp")}
                    onChange={updateAppConfigDir}
                    onBrowse={browseAppConfigDir}
                    onReset={resetAppConfigDir}
                  />
                </SettingsRow>
              </SettingsCard>
            </SettingsBlock>
            <SettingsBlock
              title={t("settings.advanced.data.title")}
              help={{
                title: t("settings.advanced.data.title"),
                body: t("settings.advanced.data.description"),
              }}
            >
              <div className="rounded-panel border border-border bg-surface p-5">
                <ImportExportSection
                  status={importStatus}
                  selectedFile={selectedFile}
                  errorMessage={errorMessage}
                  backupId={backupId}
                  isImporting={isImporting}
                  onSelectFile={selectImportFile}
                  onImport={importConfig}
                  onExport={exportConfig}
                  onClear={clearSelection}
                />
              </div>
            </SettingsBlock>
            <SettingsBlock
              title={t("settings.advanced.backup.title")}
              help={{
                title: t("settings.advanced.backup.title"),
                body: t("settings.advanced.backup.description"),
              }}
            >
              <div className="rounded-panel border border-border bg-surface p-5">
                <BackupListSection
                  backupIntervalHours={settings.backupIntervalHours}
                  backupRetainCount={settings.backupRetainCount}
                  onSettingsChange={(updates) => handleAutoSave(updates)}
                />
              </div>
            </SettingsBlock>
            <SettingsBlock
              title={t("settings.advanced.cloudSync.title")}
              help={{
                title: t("settings.advanced.cloudSync.title"),
                body: t("settings.advanced.cloudSync.description"),
              }}
            >
              <div className="rounded-panel border border-border bg-surface p-5">
                <WebdavSyncSection
                  config={settings.webdavSync}
                  s3Config={settings.s3Sync}
                  settings={settings}
                  onAutoSave={handleAutoSave}
                />
              </div>
            </SettingsBlock>
            <SettingsBlock
              title={t("settings.advanced.logConfig.title")}
              help={{
                title: t("settings.advanced.logConfig.title"),
                body: t("settings.advanced.logConfig.description"),
              }}
            >
              <div className="rounded-panel border border-border bg-surface p-5">
                <LogConfigPanel />
              </div>
            </SettingsBlock>
          </>
        );
      case "about":
        return <AboutSection isPortable={isPortable} />;
    }
  };

  return (
    <>
      <AppPageHeader
        icon={<SectionIcon className="h-5 w-5" strokeWidth={1.5} />}
        title={t(`settings.sections.${section}`)}
      />
      <div
        ref={scrollRef}
        id="main-content"
        className="min-h-0 flex-1 overflow-y-auto scroll-stable"
      >
        {isBusy ? (
          <div className="flex h-full items-center justify-center">
            <Loader2 className="h-8 w-8 animate-spin text-fg-3" />
          </div>
        ) : (
          <SettingsBody>{renderSection()}</SettingsBody>
        )}
      </div>

      <Dialog
        open={showRestartPrompt}
        onOpenChange={(open) => !open && handleRestartLater()}
      >
        <DialogContent zIndex="alert" className="max-w-md">
          <DialogHeader>
            <DialogTitle>{t("settings.restartRequired")}</DialogTitle>
          </DialogHeader>
          <div className="px-6">
            <p className="text-body text-fg-2">
              {t("settings.restartRequiredMessage")}
            </p>
          </div>
          <DialogFooter>
            <Button
              variant="neutral"
              size="regular"
              onClick={handleRestartLater}
            >
              {t("settings.restartLater")}
            </Button>
            <Button
              variant="solid"
              size="regular"
              onClick={() => void handleRestartNow()}
            >
              {t("settings.restartNow")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}
