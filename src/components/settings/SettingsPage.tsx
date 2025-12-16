import { useCallback, useEffect, useMemo, useState } from "react";
import {
  Loader2,
  Save,
  FolderSearch,
  Activity,
  Coins,
  Database,
  Server,
  ChevronDown,
} from "lucide-react";
import * as AccordionPrimitive from "@radix-ui/react-accordion";
import { toast } from "sonner";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  Accordion,
  AccordionContent,
  AccordionItem,
  AccordionTrigger,
} from "@/components/ui/accordion";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Button } from "@/components/ui/button";
import { settingsApi } from "@/lib/api";
import { LanguageSettings } from "@/components/settings/LanguageSettings";
import { ThemeSettings } from "@/components/settings/ThemeSettings";
import { WindowSettings } from "@/components/settings/WindowSettings";
import { DirectorySettings } from "@/components/settings/DirectorySettings";
import { ImportExportSection } from "@/components/settings/ImportExportSection";
import { AboutSection } from "@/components/settings/AboutSection";
import { ProxyPanel } from "@/components/proxy";
import { PricingConfigPanel } from "@/components/usage/PricingConfigPanel";
import { ModelTestConfigPanel } from "@/components/usage/ModelTestConfigPanel";
import { AutoFailoverConfigPanel } from "@/components/proxy/AutoFailoverConfigPanel";
import { FailoverQueueManager } from "@/components/proxy/FailoverQueueManager";
import { UsageDashboard } from "@/components/usage/UsageDashboard";
import { useSettings } from "@/hooks/useSettings";
import { useImportExport } from "@/hooks/useImportExport";
import { useTranslation } from "react-i18next";
import type { SettingsFormState } from "@/hooks/useSettings";
import { Switch } from "@/components/ui/switch";
import { Badge } from "@/components/ui/badge";
import { useProxyStatus } from "@/hooks/useProxyStatus";

interface SettingsDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onImportSuccess?: () => void | Promise<void>;
}

export function SettingsPage({
  open,
  onOpenChange,
  onImportSuccess,
}: SettingsDialogProps) {
  const { t } = useTranslation();
  const {
    settings,
    isLoading,
    isSaving,
    isPortable,
    appConfigDir,
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

  const [activeTab, setActiveTab] = useState<string>("general");
  const [showRestartPrompt, setShowRestartPrompt] = useState(false);

  useEffect(() => {
    if (open) {
      setActiveTab("general");
      resetStatus();
    }
  }, [open, resetStatus]);

  useEffect(() => {
    if (requiresRestart) {
      setShowRestartPrompt(true);
    }
  }, [requiresRestart]);

  const closeAfterSave = useCallback(() => {
    // 保存成功后关闭：不再重置语言，避免需要“保存两次”才生效
    acknowledgeRestart();
    clearSelection();
    resetStatus();
    onOpenChange(false);
  }, [acknowledgeRestart, clearSelection, onOpenChange, resetStatus]);

  const handleSave = useCallback(async () => {
    try {
      const result = await saveSettings(undefined, { silent: false });
      if (!result) return;
      if (result.requiresRestart) {
        setShowRestartPrompt(true);
        return;
      }
      closeAfterSave();
    } catch (error) {
      console.error("[SettingsPage] Failed to save settings", error);
    }
  }, [closeAfterSave, saveSettings]);

  const handleRestartLater = useCallback(() => {
    setShowRestartPrompt(false);
    closeAfterSave();
  }, [closeAfterSave]);

  const handleRestartNow = useCallback(async () => {
    setShowRestartPrompt(false);
    if (import.meta.env.DEV) {
      toast.success(t("settings.devModeRestartHint"), { closeButton: true });
      closeAfterSave();
      return;
    }

    try {
      await settingsApi.restart();
    } catch (error) {
      console.error("[SettingsPage] Failed to restart app", error);
      toast.error(t("settings.restartFailed"));
    } finally {
      closeAfterSave();
    }
  }, [closeAfterSave, t]);

  // 通用设置即时保存（无需手动点击）
  // 使用 autoSaveSettings 避免误触发系统 API（开机自启、Claude 插件等）
  const handleAutoSave = useCallback(
    async (updates: Partial<SettingsFormState>) => {
      if (!settings) return;
      updateSettings(updates);
      try {
        await autoSaveSettings(updates);
      } catch (error) {
        console.error("[SettingsPage] Failed to autosave settings", error);
        toast.error(
          t("settings.saveFailedGeneric", {
            defaultValue: "保存失败，请重试",
          }),
        );
      }
    },
    [autoSaveSettings, settings, t, updateSettings],
  );

  const isBusy = useMemo(() => isLoading && !settings, [isLoading, settings]);

  const {
    isRunning,
    startWithTakeover: startProxy,
    stopWithRestore: stopProxy,
    isPending: isProxyPending,
  } = useProxyStatus();
  const [failoverEnabled, setFailoverEnabled] = useState(true);

  const handleToggleProxy = async (checked: boolean) => {
    try {
      if (!checked) {
        await stopProxy();
      } else {
        await startProxy();
      }
    } catch (error) {
      console.error("Toggle proxy failed:", error);
    }
  };

  return (
    <div className="mx-auto max-w-[56rem] flex flex-col h-[calc(100vh-8rem)] overflow-hidden px-6">
      {isBusy ? (
        <div className="flex flex-1 items-center justify-center">
          <Loader2 className="h-8 w-8 animate-spin text-muted-foreground" />
        </div>
      ) : (
        <Tabs
          value={activeTab}
          onValueChange={setActiveTab}
          className="flex flex-col h-full"
        >
          <TabsList className="grid w-full grid-cols-4 mb-6 glass rounded-lg">
            <TabsTrigger value="general">
              {t("settings.tabGeneral")}
            </TabsTrigger>
            <TabsTrigger value="advanced">
              {t("settings.tabAdvanced")}
            </TabsTrigger>
            <TabsTrigger value="usage">
              {t("usage.title", "使用统计")}
            </TabsTrigger>
            <TabsTrigger value="about">{t("common.about")}</TabsTrigger>
          </TabsList>

          <div className="flex-1 overflow-y-auto overflow-x-hidden pr-2">
            <TabsContent value="general" className="space-y-6 mt-0">
              {settings ? (
                <>
                  <LanguageSettings
                    value={settings.language}
                    onChange={(lang) => handleAutoSave({ language: lang })}
                  />
                  <ThemeSettings />
                  <WindowSettings
                    settings={settings}
                    onChange={handleAutoSave}
                  />
                </>
              ) : null}
            </TabsContent>

            <TabsContent value="advanced" className="space-y-6 mt-0 pb-6">
              {settings ? (
                <div className="space-y-4">
                  <Accordion
                    type="multiple"
                    defaultValue={[]}
                    className="w-full space-y-4"
                  >
                    <AccordionItem
                      value="directory"
                      className="rounded-xl glass-card overflow-hidden"
                    >
                      <AccordionTrigger className="px-6 py-4 hover:no-underline hover:bg-muted/50 data-[state=open]:bg-muted/50">
                        <div className="flex items-center gap-3">
                          <FolderSearch className="h-5 w-5 text-primary" />
                          <div className="text-left">
                            <h3 className="text-base font-semibold">
                              配置文件目录
                            </h3>
                            <p className="text-sm text-muted-foreground font-normal">
                              管理 Claude、Codex 和 Gemini 的配置存储路径
                            </p>
                          </div>
                        </div>
                      </AccordionTrigger>
                      <AccordionContent className="px-6 pb-6 pt-4 border-t border-border/50">
                        <DirectorySettings
                          appConfigDir={appConfigDir}
                          resolvedDirs={resolvedDirs}
                          onAppConfigChange={updateAppConfigDir}
                          onBrowseAppConfig={browseAppConfigDir}
                          onResetAppConfig={resetAppConfigDir}
                          claudeDir={settings.claudeConfigDir}
                          codexDir={settings.codexConfigDir}
                          geminiDir={settings.geminiConfigDir}
                          onDirectoryChange={updateDirectory}
                          onBrowseDirectory={browseDirectory}
                          onResetDirectory={resetDirectory}
                        />
                      </AccordionContent>
                    </AccordionItem>

                    <AccordionItem
                      value="proxy"
                      className="rounded-xl glass-card overflow-hidden [&[data-state=open]>.accordion-header]:bg-muted/50"
                    >
                      <AccordionPrimitive.Header className="accordion-header flex items-center justify-between px-6 py-4 hover:bg-muted/50">
                        <AccordionPrimitive.Trigger className="flex flex-1 items-center justify-between hover:no-underline [&[data-state=open]>svg]:rotate-180">
                          <div className="flex items-center gap-3">
                            <Server className="h-5 w-5 text-green-500" />
                            <div className="text-left">
                              <h3 className="text-base font-semibold">
                                本地代理
                              </h3>
                              <p className="text-sm text-muted-foreground font-normal">
                                控制代理服务开关、查看状态与端口信息
                              </p>
                            </div>
                          </div>
                          <ChevronDown className="h-4 w-4 shrink-0 transition-transform duration-200" />
                        </AccordionPrimitive.Trigger>

                        <div className="flex items-center gap-4 pl-4">
                          <Badge
                            variant={isRunning ? "default" : "secondary"}
                            className="gap-1.5 h-6"
                          >
                            <Activity
                              className={`h-3 w-3 ${isRunning ? "animate-pulse" : ""}`}
                            />
                            {isRunning ? "运行中" : "已停止"}
                          </Badge>
                          <Switch
                            checked={isRunning}
                            onCheckedChange={handleToggleProxy}
                            disabled={isProxyPending}
                          />
                        </div>
                      </AccordionPrimitive.Header>
                      <AccordionContent className="px-6 pb-6 pt-0 border-t border-border/50">
                        <ProxyPanel />
                      </AccordionContent>
                    </AccordionItem>

                    <AccordionItem
                      value="test"
                      className="rounded-xl glass-card overflow-hidden"
                    >
                      <AccordionTrigger className="px-6 py-4 hover:no-underline hover:bg-muted/50 data-[state=open]:bg-muted/50">
                        <div className="flex items-center gap-3">
                          <Activity className="h-5 w-5 text-indigo-500" />
                          <div className="text-left">
                            <h3 className="text-base font-semibold">
                              模型测试配置
                            </h3>
                            <p className="text-sm text-muted-foreground font-normal">
                              配置模型测试使用的默认模型和提示词
                            </p>
                          </div>
                        </div>
                      </AccordionTrigger>
                      <AccordionContent className="px-6 pb-6 pt-4 border-t border-border/50">
                        <ModelTestConfigPanel />
                      </AccordionContent>
                    </AccordionItem>

                    <AccordionItem
                      value="failover"
                      className="rounded-xl glass-card overflow-hidden [&[data-state=open]>.accordion-header]:bg-muted/50"
                    >
                      <AccordionPrimitive.Header className="accordion-header flex items-center justify-between px-6 py-4 hover:bg-muted/50">
                        <AccordionPrimitive.Trigger className="flex flex-1 items-center justify-between hover:no-underline [&[data-state=open]>svg]:rotate-180">
                          <div className="flex items-center gap-3">
                            <Activity className="h-5 w-5 text-orange-500" />
                            <div className="text-left">
                              <h3 className="text-base font-semibold">
                                自动故障转移
                              </h3>
                              <p className="text-sm text-muted-foreground font-normal">
                                配置故障转移队列和熔断策略
                              </p>
                            </div>
                          </div>
                          <ChevronDown className="h-4 w-4 shrink-0 transition-transform duration-200" />
                        </AccordionPrimitive.Trigger>

                        <div className="flex items-center gap-2 pl-4">
                          <Switch
                            checked={failoverEnabled}
                            onCheckedChange={setFailoverEnabled}
                          />
                        </div>
                      </AccordionPrimitive.Header>
                      <AccordionContent className="px-6 pb-6 pt-4 border-t border-border/50">
                        <div className="space-y-6">
                          {/* 故障转移队列管理 */}
                          <div className="space-y-4">
                            <div>
                              <h4 className="text-sm font-semibold">
                                {t("proxy.failoverQueue.title", "故障转移队列")}
                              </h4>
                              <p className="text-xs text-muted-foreground">
                                {t(
                                  "proxy.failoverQueue.description",
                                  "管理各应用的供应商故障转移顺序",
                                )}
                              </p>
                            </div>
                            <Tabs defaultValue="claude" className="w-full">
                              <TabsList className="grid w-full grid-cols-3">
                                <TabsTrigger value="claude">Claude</TabsTrigger>
                                <TabsTrigger value="codex">Codex</TabsTrigger>
                                <TabsTrigger value="gemini">Gemini</TabsTrigger>
                              </TabsList>
                              <TabsContent value="claude" className="mt-4">
                                <FailoverQueueManager
                                  appType="claude"
                                  disabled={!failoverEnabled}
                                />
                              </TabsContent>
                              <TabsContent value="codex" className="mt-4">
                                <FailoverQueueManager
                                  appType="codex"
                                  disabled={!failoverEnabled}
                                />
                              </TabsContent>
                              <TabsContent value="gemini" className="mt-4">
                                <FailoverQueueManager
                                  appType="gemini"
                                  disabled={!failoverEnabled}
                                />
                              </TabsContent>
                            </Tabs>
                          </div>

                          {/* 熔断器配置 */}
                          <div className="border-t border-border/50 pt-6">
                            <AutoFailoverConfigPanel
                              enabled={failoverEnabled}
                              onEnabledChange={setFailoverEnabled}
                            />
                          </div>
                        </div>
                      </AccordionContent>
                    </AccordionItem>

                    <AccordionItem
                      value="pricing"
                      className="rounded-xl glass-card overflow-hidden"
                    >
                      <AccordionTrigger className="px-6 py-4 hover:no-underline hover:bg-muted/50 data-[state=open]:bg-muted/50">
                        <div className="flex items-center gap-3">
                          <Coins className="h-5 w-5 text-yellow-500" />
                          <div className="text-left">
                            <h3 className="text-base font-semibold">
                              成本定价
                            </h3>
                            <p className="text-sm text-muted-foreground font-normal">
                              管理各模型 Token 计费规则
                            </p>
                          </div>
                        </div>
                      </AccordionTrigger>
                      <AccordionContent className="px-6 pb-6 pt-4 border-t border-border/50">
                        <PricingConfigPanel />
                      </AccordionContent>
                    </AccordionItem>

                    <AccordionItem
                      value="data"
                      className="rounded-xl glass-card overflow-hidden"
                    >
                      <AccordionTrigger className="px-6 py-4 hover:no-underline hover:bg-muted/50 data-[state=open]:bg-muted/50">
                        <div className="flex items-center gap-3">
                          <Database className="h-5 w-5 text-blue-500" />
                          <div className="text-left">
                            <h3 className="text-base font-semibold">
                              数据管理
                            </h3>
                            <p className="text-sm text-muted-foreground font-normal">
                              导入导出配置与备份恢复
                            </p>
                          </div>
                        </div>
                      </AccordionTrigger>
                      <AccordionContent className="px-6 pb-6 pt-4 border-t border-border/50">
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
                      </AccordionContent>
                    </AccordionItem>
                  </Accordion>

                  <div className="pt-4">
                    <Button
                      onClick={handleSave}
                      className="w-full h-12 text-base font-medium"
                      disabled={isSaving}
                    >
                      {isSaving ? (
                        <span className="inline-flex items-center gap-2">
                          <Loader2 className="h-5 w-5 animate-spin" />
                          {t("settings.saving")}
                        </span>
                      ) : (
                        <>
                          <Save className="mr-2 h-5 w-5" />
                          {t("common.save")}
                        </>
                      )}
                    </Button>
                  </div>
                </div>
              ) : null}
            </TabsContent>

            <TabsContent value="about" className="mt-0">
              <AboutSection isPortable={isPortable} />
            </TabsContent>

            <TabsContent value="usage" className="mt-0">
              <UsageDashboard />
            </TabsContent>
          </div>
        </Tabs>
      )}

      <Dialog
        open={showRestartPrompt}
        onOpenChange={(open) => !open && handleRestartLater()}
      >
        <DialogContent zIndex="alert" className="max-w-md glass border-border">
          <DialogHeader>
            <DialogTitle>{t("settings.restartRequired")}</DialogTitle>
          </DialogHeader>
          <div className="px-6">
            <p className="text-sm text-muted-foreground">
              {t("settings.restartRequiredMessage")}
            </p>
          </div>
          <DialogFooter>
            <Button
              variant="ghost"
              onClick={handleRestartLater}
              className="hover:bg-muted/50"
            >
              {t("settings.restartLater")}
            </Button>
            <Button
              onClick={handleRestartNow}
              className="bg-primary hover:bg-primary/90"
            >
              {t("settings.restartNow")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
