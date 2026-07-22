import { useTranslation } from "react-i18next";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  useCodexWorkbenchSettingsQuery,
  useCodexWorkbenchStatusQuery,
} from "@/lib/query/codexWorkbench";

/**
 * Workbench overview: runtime + enhancement-level prompt/continuation flags.
 * Never loads or displays prompt text — only booleans / ids / fingerprints.
 */
export function OverviewTab() {
  const { t } = useTranslation();
  const statusQ = useCodexWorkbenchStatusQuery(true);
  const settingsQ = useCodexWorkbenchSettingsQuery(true);

  const status = statusQ.data;
  const settings = settingsQ.data;
  const enhancements = settings?.enhancements;

  const cdpConnected = status?.cdpPort != null && status.cdpPort > 0;
  const bridgeReady =
    (status?.bridgeState ?? "").toLowerCase().includes("ready") ||
    (status?.bridgeState ?? "").toLowerCase().includes("connected");
  const promptEnabled = Boolean(enhancements?.systemPrompt);
  const continuationEnabled = Boolean(enhancements?.reasoningResume);
  const providerName =
    status?.currentProviderId ??
    t("codexWorkbench.overview.noProvider", {
      defaultValue: "未选择 Provider",
    });

  return (
    <div className="space-y-4 p-1" data-testid="codex-overview-tab">
      <section className="space-y-2 rounded-md border p-3">
        <h3 className="text-sm font-medium">
          {t("codexWorkbench.overview.runtime", { defaultValue: "运行时" })}
        </h3>
        <div className="flex flex-wrap gap-2 text-sm">
          <Badge variant={cdpConnected ? "default" : "secondary"}>
            CDP{" "}
            {cdpConnected
              ? t("codexWorkbench.overview.on", { defaultValue: "已连接" })
              : t("codexWorkbench.overview.off", { defaultValue: "未连接" })}
            {cdpConnected ? ` :${status?.cdpPort}` : ""}
          </Badge>
          <Badge variant={bridgeReady ? "default" : "secondary"}>
            Bridge{" "}
            {bridgeReady
              ? t("codexWorkbench.overview.on", { defaultValue: "已连接" })
              : t("codexWorkbench.overview.off", { defaultValue: "未连接" })}
          </Badge>
          <Badge variant={status?.proxyRunning ? "default" : "secondary"}>
            Proxy {status?.proxyRunning ? "ON" : "OFF"}
          </Badge>
          {status?.runtimeState && (
            <span className="text-muted-foreground font-mono text-xs">
              {status.runtimeState}
            </span>
          )}
          {status?.bridgeState && (
            <span className="text-muted-foreground font-mono text-xs">
              bridge={status.bridgeState}
            </span>
          )}
        </div>
        {status?.lastError && (
          <p className="text-xs text-destructive">{status.lastError}</p>
        )}
      </section>

      <section className="space-y-2 rounded-md border p-3">
        <div className="flex flex-wrap items-center justify-between gap-2">
          <h3 className="text-sm font-medium">
            {t("codexWorkbench.overview.providerTitle", {
              defaultValue: "当前 Provider 提示词 / 续接",
            })}
          </h3>
          <Button
            type="button"
            size="sm"
            variant="outline"
            onClick={() => {
              window.location.hash = "#/settings/providers";
            }}
          >
            {t("codexWorkbench.overview.editProvider", {
              defaultValue: "编辑 Provider",
            })}
          </Button>
        </div>
        <p className="text-sm">
          <span className="text-muted-foreground">
            {t("codexWorkbench.overview.provider", {
              defaultValue: "Provider",
            })}
            :{" "}
          </span>
          <span className="font-medium font-mono">{providerName}</span>
        </p>
        <ul className="space-y-1 text-sm">
          <li>
            {t("codexWorkbench.overview.promptReplace", {
              defaultValue: "提示词替换",
            })}
            :{" "}
            <Badge variant={promptEnabled ? "default" : "secondary"}>
              {promptEnabled
                ? t("codexWorkbench.overview.enabled", { defaultValue: "开" })
                : t("codexWorkbench.overview.disabled", { defaultValue: "关" })}
            </Badge>
          </li>
          <li>
            {t("codexWorkbench.overview.continuation", {
              defaultValue: "推理续接",
            })}
            :{" "}
            <Badge variant={continuationEnabled ? "default" : "secondary"}>
              {continuationEnabled
                ? t("codexWorkbench.overview.enabled", { defaultValue: "开" })
                : t("codexWorkbench.overview.disabled", { defaultValue: "关" })}
            </Badge>
          </li>
          {enhancements?.reasoningToken != null && (
            <li>
              reasoning token:{" "}
              <Badge
                variant={enhancements.reasoningToken ? "default" : "outline"}
              >
                {enhancements.reasoningToken ? "on" : "off"}
              </Badge>
            </li>
          )}
        </ul>
        <p className="text-xs text-muted-foreground">
          {t("codexWorkbench.overview.noPromptText", {
            defaultValue: "状态卡不读取、不展示任何提示词正文。",
          })}
        </p>
      </section>

      <section className="space-y-2 rounded-md border p-3">
        <h3 className="text-sm font-medium">
          {t("codexWorkbench.overview.enhancements", {
            defaultValue: "页面增强摘要",
          })}
        </h3>
        <p className="text-xs text-muted-foreground">
          {t("codexWorkbench.overview.enhancementsHint", {
            defaultValue: "详细开关见「增强」页签；此处仅作总览。",
          })}
        </p>
        {enhancements ? (
          <div className="flex flex-wrap gap-1">
            {Object.entries(enhancements)
              .filter(([, v]) => typeof v === "boolean")
              .map(([k, v]) => (
                <Badge key={k} variant={v ? "default" : "outline"}>
                  {k}
                </Badge>
              ))}
          </div>
        ) : (
          <p className="text-sm text-muted-foreground">—</p>
        )}
      </section>
    </div>
  );
}

export default OverviewTab;
