import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { DeepLinkImportRequest } from "../../lib/api/deeplink";
import { decodeBase64Utf8 } from "../../lib/utils/base64";
import {
  classifyCommand,
  classifyEndpoint,
  classifyEnvKey,
  maskValue,
  riskI18nKey,
  type RiskKind,
} from "@/utils/deeplinkRisk";

export function McpConfirmation({
  request,
}: {
  request: DeepLinkImportRequest;
}) {
  const { t } = useTranslation();

  const mcpServers = useMemo(() => {
    if (!request.config) return null;
    try {
      const decoded = decodeBase64Utf8(request.config);
      const parsed = JSON.parse(decoded);
      return parsed.mcpServers || {};
    } catch (e) {
      console.error("Failed to parse MCP config:", e);
      return null;
    }
  }, [request.config]);

  const targetApps = request.apps?.split(",") || [];
  const serverCount = Object.keys(mcpServers || {}).length;

  // 汇总所有条目命中的风险，在底部给一句总的提示——逐行的 ⚠ 容易被划过去。
  const risks = useMemo(() => {
    const found = new Set<RiskKind>();
    for (const spec of Object.values(mcpServers || {}) as any[]) {
      const commandRisk = classifyCommand(spec?.command, spec?.args);
      if (commandRisk) found.add(commandRisk);
      if (typeof spec?.url === "string") {
        const urlRisk = classifyEndpoint(spec.url);
        if (urlRisk) found.add(urlRisk);
      }
      for (const key of Object.keys(spec?.env || {})) {
        const envRisk = classifyEnvKey(key);
        if (envRisk) found.add(envRisk);
      }
    }
    return [...found];
  }, [mcpServers]);

  /** 一行 key/value。`break-all` 而非 `truncate`：payload 不得被 CSS 藏起来。 */
  const Row = ({
    label,
    value,
    risk,
  }: {
    label: string;
    value: string;
    risk?: RiskKind | null;
  }) => (
    <div className="grid grid-cols-[4rem_1fr] gap-2 text-xs">
      <span className="text-muted-foreground shrink-0">{label}</span>
      <span
        className={`font-mono break-all ${
          risk ? "text-yellow-700 dark:text-yellow-500 font-semibold" : ""
        }`}
      >
        {risk && <span aria-hidden="true">⚠ </span>}
        {value}
      </span>
    </div>
  );

  return (
    <div className="space-y-4">
      <h3 className="text-lg font-semibold">{t("deeplink.mcp.title")}</h3>

      <div>
        <label className="block text-sm font-medium text-muted-foreground">
          {t("deeplink.mcp.targetApps")}
        </label>
        <div className="mt-1 flex gap-2 flex-wrap">
          {targetApps.map((app) => (
            <span
              key={app}
              className="px-2 py-1 bg-primary/10 text-primary text-xs rounded capitalize"
            >
              {app.trim()}
            </span>
          ))}
        </div>
      </div>

      <div>
        <label className="block text-sm font-medium text-muted-foreground">
          {t("deeplink.mcp.serverCount", { count: serverCount })}
        </label>
        <div className="mt-1 space-y-2 max-h-64 overflow-auto border rounded p-2 bg-muted/30">
          {mcpServers &&
            Object.entries(mcpServers).map(([id, spec]: [string, any]) => {
              const commandRisk = classifyCommand(spec?.command, spec?.args);
              const argv: string[] = Array.isArray(spec?.args)
                ? spec.args.map(String)
                : [];
              const env: Record<string, unknown> = spec?.env || {};

              return (
                <div key={id} className="p-2 bg-background rounded border">
                  <div className="font-semibold text-sm mb-1">{id}</div>
                  <div className="space-y-1">
                    {spec?.command && (
                      <Row
                        label={t("deeplink.mcp.command")}
                        value={String(spec.command)}
                        risk={commandRisk}
                      />
                    )}
                    {/* 逐项展开而不是 join(" ")：payload 常常整条藏在某一个 arg 里，
                        拼成一行再 truncate 正是它此前得以隐身的原因。 */}
                    {argv.map((arg, index) => (
                      <Row
                        key={index}
                        label={index === 0 ? t("deeplink.mcp.args") : ""}
                        value={arg}
                        risk={commandRisk}
                      />
                    ))}
                    {spec?.url && (
                      <Row
                        label={t("deeplink.mcp.url")}
                        value={String(spec.url)}
                        risk={classifyEndpoint(String(spec.url))}
                      />
                    )}
                    {Object.entries(env).map(([key, value], index) => (
                      <Row
                        key={key}
                        label={index === 0 ? t("deeplink.mcp.env") : ""}
                        value={`${key}=${maskValue(key, String(value))}`}
                        risk={classifyEnvKey(key)}
                      />
                    ))}
                  </div>
                </div>
              );
            })}
        </div>
      </div>

      {risks.length > 0 && (
        <div className="rounded border border-yellow-500/40 bg-yellow-500/10 p-2 space-y-1">
          {risks.map((kind) => (
            <div
              key={kind}
              className="text-yellow-700 dark:text-yellow-500 text-sm flex items-start gap-2"
            >
              <span aria-hidden="true">⚠️</span>
              <span>{t(riskI18nKey(kind))}</span>
            </div>
          ))}
        </div>
      )}

      {/*
        无条件显示，不看 `request.enabled`。
        MCP 导入路径**根本不读这个字段**——`deeplink/mcp.rs` 里全文没有它，
        而 `:196` 是无条件的 `merged.set_enabled_for(&app, true)`。
        （prompt.rs / skill.rs / provider.rs 各自读了 `request.enabled`，唯独 MCP 没有，
        所以很容易误以为这里也生效。）
        挂条件的后果是：恶意链接省略 `enabled` 就能让这条警告消失，而写入行为
        一模一样——把提示变成了可被攻击者关掉的开关。
      */}
      <div className="text-yellow-600 dark:text-yellow-500 text-sm flex items-center gap-2">
        <span>⚠️</span>
        <span>{t("deeplink.mcp.enabledWarning")}</span>
      </div>
    </div>
  );
}
