import { useTranslation } from "react-i18next";
import {
  useCodexWorkbenchSettingsQuery,
  useCodexWorkbenchStatusQuery,
  useLaunchEnhancedCodex,
  useReinjectCodexEnhancements,
  useUpdateCodexWorkbenchSettings,
} from "@/lib/query/codexWorkbench";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import type { CodexEnhancementSettings } from "@/types/codexWorkbench";

/**
 * Codex 工作台壳层页面。
 * 四个分区：enhancements / scripts / plugins / radar。
 * 后续任务填充后三个分区，不改导航结构。
 */
export function CodexWorkbenchPage() {
  const { t } = useTranslation();
  const statusQuery = useCodexWorkbenchStatusQuery(true);
  const settingsQuery = useCodexWorkbenchSettingsQuery(true);
  const updateSettings = useUpdateCodexWorkbenchSettings();
  const launchMut = useLaunchEnhancedCodex();
  const reinjectMut = useReinjectCodexEnhancements();

  const status = statusQuery.data;
  const settings = settingsQuery.data;

  const toggleEnhancement = (key: keyof CodexEnhancementSettings) => {
    if (!settings) return;
    updateSettings.mutate({
      ...settings,
      enhancements: {
        ...settings.enhancements,
        [key]: !settings.enhancements[key],
      },
    });
  };

  const enhancementEntries: Array<{
    key: keyof CodexEnhancementSettings;
    label: string;
  }> = [
    { key: "pluginUnlock", label: "插件解锁" },
    { key: "autoExpand", label: "自动展开" },
    { key: "sessionDelete", label: "会话删除" },
    { key: "wideConversation", label: "宽对话" },
    { key: "nativeMenu", label: "原生菜单" },
    { key: "userScriptRuntime", label: "用户脚本" },
    { key: "markdownExport", label: "Markdown 导出" },
    { key: "modelSwitcher", label: "模型切换" },
    { key: "systemPrompt", label: "系统提示" },
    { key: "reasoningResume", label: "推理恢复" },
    { key: "reasoningToken", label: "推理 Token" },
  ];

  return (
    <div className="flex h-full flex-col gap-4 p-4">
      <div className="flex flex-wrap items-center gap-2">
        <h2 className="text-lg font-semibold">
          {t("codexWorkbench.title", { defaultValue: "Codex 工作台" })}
        </h2>
        {status && (
          <>
            <Badge variant="outline">
              {status.runtimeState}
            </Badge>
            <Badge variant="secondary">
              bridge: {status.bridgeState}
            </Badge>
            {status.cdpPort != null && (
              <Badge variant="secondary">CDP {status.cdpPort}</Badge>
            )}
            {status.proxyRunning && (
              <Badge variant="default">proxy</Badge>
            )}
          </>
        )}
        <div className="ml-auto flex gap-2">
          <Button
            size="sm"
            onClick={() => launchMut.mutate()}
            disabled={
              launchMut.isPending ||
              status?.platformSupported === false ||
              status?.runtimeState === "ordinary_running"
            }
          >
            {t("codexWorkbench.launch", { defaultValue: "启动增强 Codex" })}
          </Button>
          <Button
            size="sm"
            variant="outline"
            onClick={() => reinjectMut.mutate()}
            disabled={
              reinjectMut.isPending ||
              !status?.cdpPort ||
              status?.runtimeState === "ordinary_running"
            }
          >
            {t("codexWorkbench.reinject", { defaultValue: "重新注入" })}
          </Button>
        </div>
      </div>

      {(status?.lastError ||
        launchMut.error ||
        reinjectMut.error ||
        status?.runtimeState === "ordinary_running") && (
        <div className="rounded-md border border-amber-500/40 bg-amber-500/10 px-3 py-2 text-sm">
          {status?.runtimeState === "ordinary_running" && (
            <p>
              {t("codexWorkbench.ordinaryRunningHint", {
                defaultValue:
                  "检测到已运行的普通 Codex。请先手动关闭后再启动增强模式（不会强制结束进程）。",
              })}
            </p>
          )}
          {status?.lastError && <p>{status.lastError}</p>}
          {launchMut.error && (
            <p className="text-destructive">
              {(launchMut.error as Error).message || String(launchMut.error)}
            </p>
          )}
          {reinjectMut.error && (
            <p className="text-destructive">
              {(reinjectMut.error as Error).message ||
                String(reinjectMut.error)}
            </p>
          )}
        </div>
      )}

      <Tabs defaultValue="enhancements" className="flex min-h-0 flex-1 flex-col">
        <TabsList className="w-fit">
          <TabsTrigger value="enhancements">
            {t("codexWorkbench.tabs.enhancements", {
              defaultValue: "增强",
            })}
          </TabsTrigger>
          <TabsTrigger value="scripts">
            {t("codexWorkbench.tabs.scripts", { defaultValue: "脚本" })}
          </TabsTrigger>
          <TabsTrigger value="plugins">
            {t("codexWorkbench.tabs.plugins", { defaultValue: "插件" })}
          </TabsTrigger>
          <TabsTrigger value="radar">
            {t("codexWorkbench.tabs.radar", { defaultValue: "降智雷达" })}
          </TabsTrigger>
        </TabsList>

        <TabsContent value="enhancements" className="flex-1 overflow-auto">
          <div className="space-y-3 rounded-lg border p-4 text-sm">
            <p className="text-muted-foreground">
              {t("codexWorkbench.enhancementsHint", {
                defaultValue:
                  "增强开关会写入本地设置，并在启动/重新注入时注入到 Codex 页面。",
              })}
            </p>
            <div className="grid gap-2 sm:grid-cols-2 lg:grid-cols-3">
              {enhancementEntries.map(({ key, label }) => {
                const on = settings?.enhancements?.[key] ?? false;
                return (
                  <button
                    key={key}
                    type="button"
                    className="flex items-center justify-between rounded-md border px-3 py-2 text-left hover:bg-muted/50"
                    onClick={() => toggleEnhancement(key)}
                    disabled={!settings || updateSettings.isPending}
                  >
                    <span>{label}</span>
                    <Badge variant={on ? "default" : "outline"}>
                      {on ? "ON" : "OFF"}
                    </Badge>
                  </button>
                );
              })}
            </div>
          </div>
        </TabsContent>

        <TabsContent value="scripts" className="flex-1 overflow-auto">
          <div className="rounded-lg border p-4 text-sm text-muted-foreground">
            {t("codexWorkbench.scriptsPlaceholder", {
              defaultValue: "脚本市场将在后续任务接入。",
            })}
          </div>
        </TabsContent>

        <TabsContent value="plugins" className="flex-1 overflow-auto">
          <div className="rounded-lg border p-4 text-sm text-muted-foreground">
            {t("codexWorkbench.pluginsPlaceholder", {
              defaultValue: "插件管理将在后续任务接入。",
            })}
          </div>
        </TabsContent>

        <TabsContent value="radar" className="flex-1 overflow-auto">
          <div className="rounded-lg border p-4 text-sm text-muted-foreground">
            {t("codexWorkbench.radarPlaceholder", {
              defaultValue: "降智雷达将在后续任务接入。",
            })}
          </div>
        </TabsContent>
      </Tabs>
    </div>
  );
}

export default CodexWorkbenchPage;
