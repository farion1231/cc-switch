import { useTranslation } from "react-i18next";
import {
  useCodexWorkbenchSettingsQuery,
  useCodexWorkbenchStatusQuery,
} from "@/lib/query/codexWorkbench";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Badge } from "@/components/ui/badge";

/**
 * Codex 工作台壳层页面。
 * 四个分区：enhancements / scripts / plugins / radar。
 * 后续任务填充后三个分区，不改导航结构。
 */
export function CodexWorkbenchPage() {
  const { t } = useTranslation();
  const statusQuery = useCodexWorkbenchStatusQuery({
    enabled: true,
    refetchInterval: 2000,
  });
  const settingsQuery = useCodexWorkbenchSettingsQuery(true);

  const status = statusQuery.data;
  const settings = settingsQuery.data;

  return (
    <div className="flex h-full flex-col gap-4 p-4">
      <div className="flex flex-wrap items-center gap-2">
        <h2 className="text-lg font-semibold">
          {t("codexWorkbench.title", { defaultValue: "Codex 工作台" })}
        </h2>
        {status && (
          <>
            <Badge variant="secondary">
              {t("codexWorkbench.runtime", { defaultValue: "运行时" })}:{" "}
              {status.runtimeState}
            </Badge>
            <Badge variant={status.proxyRunning ? "default" : "outline"}>
              proxy: {status.proxyRunning ? "on" : "off"}
            </Badge>
            <Badge variant="outline">
              {status.platformSupported ? "Windows" : "unsupported"}
            </Badge>
            {status.currentProviderId && (
              <Badge variant="outline">
                provider: {status.currentProviderId}
              </Badge>
            )}
          </>
        )}
        {statusQuery.isError && (
          <span className="text-sm text-destructive">
            {t("codexWorkbench.statusError", {
              defaultValue: "状态加载失败",
            })}
          </span>
        )}
      </div>

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
          <div className="space-y-2 rounded-lg border p-4 text-sm">
            <p className="text-muted-foreground">
              {t("codexWorkbench.enhancementsHint", {
                defaultValue:
                  "增强开关矩阵（后续任务接开关控件）。默认：前 6 项开启，后 5 项关闭。",
              })}
            </p>
            {settings && (
              <ul className="grid grid-cols-1 gap-1 sm:grid-cols-2">
                {Object.entries(settings.enhancements).map(([key, value]) => (
                  <li
                    key={key}
                    className="flex items-center justify-between rounded bg-muted/40 px-2 py-1"
                  >
                    <span className="font-mono text-xs">{key}</span>
                    <Badge variant={value ? "default" : "outline"}>
                      {value ? "on" : "off"}
                    </Badge>
                  </li>
                ))}
              </ul>
            )}
            {settings && (
              <div className="mt-3 space-y-1 text-xs text-muted-foreground">
                <div>autoLaunch: {String(settings.autoLaunch)}</div>
                <div>autoStartProxy: {String(settings.autoStartProxy)}</div>
                <div>radarTtlMinutes: {settings.radarTtlMinutes}</div>
                <div className="truncate">
                  scriptMarketUrl: {settings.scriptMarketUrl}
                </div>
              </div>
            )}
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
