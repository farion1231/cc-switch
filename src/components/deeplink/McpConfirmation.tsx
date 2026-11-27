import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { getAppLabel } from "@/config/appConfig";
import { DeepLinkImportRequest } from "../../lib/api/deeplink";
import { decodeBase64Utf8 } from "../../lib/utils/base64";
import {
  classifyCommand,
  classifyEndpoint,
  classifyEnvKey,
  classifyMcpField,
  containsEnvVarReference,
  maskValue,
  riskI18nKey,
  type RiskKind,
} from "@/utils/deeplinkRisk";

const KNOWN_MCP_FIELDS = new Set([
  "type",
  "command",
  "args",
  "cwd",
  "url",
  "headers",
  "http_headers",
  "env",
]);

const MCP_URL_FIELDS = new Set(["url", "httpurl", "serverurl"]);

function classifyUrlValue(value: unknown): RiskKind | null {
  if (containsEnvVarReference(value)) return "mcpDangerousField";
  return classifyEndpoint(value);
}

function isPlainObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function formatScalarOrJson(value: unknown): string {
  if (typeof value === "string") return value;
  try {
    return JSON.stringify(value);
  } catch {
    return String(value);
  }
}

function formatMaskedEntryValue(key: string, value: unknown): string {
  if (typeof value === "string") {
    return maskValue(key, value);
  }
  return formatScalarOrJson(value);
}

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
      return isPlainObject(parsed?.mcpServers) ? parsed.mcpServers : {};
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
    for (const spec of Object.values(mcpServers || {})) {
      if (!isPlainObject(spec)) continue;
      const commandRisk = classifyCommand(spec.command, spec.args);
      if (commandRisk) found.add(commandRisk);
      if (isPlainObject(spec.env)) {
        for (const key of Object.keys(spec.env)) {
          const envRisk = classifyEnvKey(key);
          if (envRisk) found.add(envRisk);
        }
      }
      for (const [key, value] of Object.entries(spec)) {
        const lower = key.toLowerCase();
        const fieldRisk = classifyMcpField(key);
        if (fieldRisk) found.add(fieldRisk);
        if (MCP_URL_FIELDS.has(lower)) {
          const endpointRisk = classifyEndpoint(value);
          if (endpointRisk) found.add(endpointRisk);
          if (containsEnvVarReference(value)) {
            found.add("mcpDangerousField");
          }
        }
        if (
          (lower === "headers" || lower === "http_headers") &&
          isPlainObject(value)
        ) {
          for (const headerVal of Object.values(value)) {
            if (containsEnvVarReference(headerVal)) {
              found.add("mcpDangerousField");
            }
          }
        }
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
  }) => {
    const riskClass =
      risk === "mcpDangerousField"
        ? "text-destructive font-semibold"
        : risk
          ? "text-warning-text font-semibold"
          : "";
    return (
      <div className="grid grid-cols-[minmax(4rem,auto)_1fr] gap-2 text-xs">
        <span className={`break-all ${riskClass || "text-fg-2"}`}>{label}</span>
        <span className={`font-mono break-all ${riskClass}`}>
          {risk && <span aria-hidden="true">⚠ </span>}
          {value}
        </span>
      </div>
    );
  };

  return (
    <div className="space-y-4">
      <h3 className="text-lg font-semibold">{t("deeplink.mcp.title")}</h3>

      <div>
        <label className="block text-sm font-medium text-fg-2">
          {t("deeplink.mcp.targetApps")}
        </label>
        <div className="mt-1 flex gap-2 flex-wrap">
          {targetApps.map((app) => (
            <span
              key={app}
              className="px-2 py-1 bg-primary/10 text-primary text-xs rounded"
            >
              {getAppLabel(app.trim())}
            </span>
          ))}
        </div>
      </div>

      <div>
        <label className="block text-sm font-medium text-fg-2">
          {t("deeplink.mcp.serverCount", { count: serverCount })}
        </label>
        <div className="mt-1 space-y-2 max-h-64 overflow-auto border rounded p-2 bg-subtle">
          {mcpServers &&
            Object.entries(mcpServers).map(([id, rawSpec]) => {
              if (!isPlainObject(rawSpec)) {
                return (
                  <div key={id} className="p-2 bg-surface rounded border">
                    <div className="font-semibold text-sm mb-1">{id}</div>
                    <Row label="" value={formatScalarOrJson(rawSpec)} />
                  </div>
                );
              }

              const spec = rawSpec;
              const commandRisk = classifyCommand(spec.command, spec.args);
              const argv: string[] = Array.isArray(spec.args)
                ? spec.args.map(formatScalarOrJson)
                : [];
              const envEntries = isPlainObject(spec.env)
                ? Object.entries(spec.env)
                : [];
              const headerEntries = isPlainObject(spec.headers)
                ? Object.entries(spec.headers)
                : [];
              const httpHeaderEntries = isPlainObject(spec.http_headers)
                ? Object.entries(spec.http_headers)
                : [];
              const extraEntries = Object.entries(spec).filter(
                ([key]) => !KNOWN_MCP_FIELDS.has(key),
              );

              return (
                <div key={id} className="p-2 bg-surface rounded border">
                  <div className="font-semibold text-sm mb-1">{id}</div>
                  <div className="space-y-1">
                    {spec.type !== undefined && (
                      <Row
                        label={t("deeplink.mcp.type", { defaultValue: "Type" })}
                        value={formatScalarOrJson(spec.type)}
                      />
                    )}
                    {spec.command !== undefined && (
                      <Row
                        label={t("deeplink.mcp.command")}
                        value={formatScalarOrJson(spec.command)}
                        risk={commandRisk}
                      />
                    )}
                    {/* 逐项展开而不是 join("")：payload 常常整条藏在某一个 arg 里，
                        拼成一行再 truncate 正是它此前得以隐身的原因。 */}
                    {argv.map((arg, index) => (
                      <Row
                        key={index}
                        label={index === 0 ? t("deeplink.mcp.args") : ""}
                        value={arg}
                        risk={commandRisk}
                      />
                    ))}
                    {spec.args !== undefined && !Array.isArray(spec.args) && (
                      <Row
                        label={t("deeplink.mcp.args")}
                        value={formatScalarOrJson(spec.args)}
                      />
                    )}
                    {spec.cwd !== undefined && (
                      <Row
                        label={t("deeplink.mcp.cwd", { defaultValue: "CWD" })}
                        value={formatScalarOrJson(spec.cwd)}
                        risk={classifyMcpField("cwd")}
                      />
                    )}
                    {spec.url !== undefined && (
                      <Row
                        label={t("deeplink.mcp.url")}
                        value={formatScalarOrJson(spec.url)}
                        risk={classifyUrlValue(spec.url)}
                      />
                    )}
                    {headerEntries.map(([key, value], index) => {
                      const envRefRisk: RiskKind | null =
                        containsEnvVarReference(value)
                          ? "mcpDangerousField"
                          : null;
                      const displayValue = envRefRisk
                        ? formatScalarOrJson(value)
                        : formatMaskedEntryValue(key, value);
                      return (
                        <Row
                          key={`headers-${key}`}
                          label={
                            index === 0
                              ? t("deeplink.mcp.headers", {
                                  defaultValue: "Headers",
                                })
                              : ""
                          }
                          value={`${key}=${displayValue}`}
                          risk={envRefRisk}
                        />
                      );
                    })}
                    {spec.headers !== undefined &&
                      !isPlainObject(spec.headers) && (
                        <Row
                          label={t("deeplink.mcp.headers", {
                            defaultValue: "Headers",
                          })}
                          value={formatScalarOrJson(spec.headers)}
                        />
                      )}
                    {httpHeaderEntries.map(([key, value], index) => {
                      const envRefRisk: RiskKind | null =
                        containsEnvVarReference(value)
                          ? "mcpDangerousField"
                          : null;
                      const displayValue = envRefRisk
                        ? formatScalarOrJson(value)
                        : formatMaskedEntryValue(key, value);
                      return (
                        <Row
                          key={`http-headers-${key}`}
                          label={index === 0 ? "http_headers" : ""}
                          value={`${key}=${displayValue}`}
                          risk={envRefRisk}
                        />
                      );
                    })}
                    {spec.http_headers !== undefined &&
                      !isPlainObject(spec.http_headers) && (
                        <Row
                          label="http_headers"
                          value={formatScalarOrJson(spec.http_headers)}
                        />
                      )}
                    {envEntries.map(([key, value], index) => (
                      <Row
                        key={`env-${key}`}
                        label={index === 0 ? t("deeplink.mcp.env") : ""}
                        value={`${key}=${formatMaskedEntryValue(key, value)}`}
                        risk={classifyEnvKey(key)}
                      />
                    ))}
                    {spec.env !== undefined && !isPlainObject(spec.env) && (
                      <Row
                        label={t("deeplink.mcp.env")}
                        value={formatScalarOrJson(spec.env)}
                      />
                    )}
                    {/* 完整展示其余所有扩展/未知字段，绝不静默丢弃；命中高危字段时原样展示（不脱敏环境变量名）并标红告警。 */}
                    {extraEntries.map(([key, value]) => {
                      const fieldRisk =
                        classifyMcpField(key) ??
                        (MCP_URL_FIELDS.has(key.toLowerCase())
                          ? classifyUrlValue(value)
                          : null);
                      const displayValue =
                        fieldRisk === "mcpDangerousField"
                          ? formatScalarOrJson(value)
                          : formatMaskedEntryValue(key, value);
                      return (
                        <Row
                          key={`extra-${key}`}
                          label={key}
                          value={displayValue}
                          risk={fieldRisk}
                        />
                      );
                    })}
                  </div>
                </div>
              );
            })}
        </div>
      </div>

      {risks.length > 0 && (
        <div className="rounded border border-transparent bg-warning-soft p-2 space-y-1">
          {risks.map((kind) => (
            <div
              key={kind}
              className={`text-sm flex items-start gap-2 ${
                kind === "mcpDangerousField"
                  ? "text-destructive font-medium"
                  : "text-warning-text"
              }`}
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
      <div className="text-warning-text text-sm flex items-center gap-2">
        <span>⚠️</span>
        <span>{t("deeplink.mcp.enabledWarning")}</span>
      </div>
    </div>
  );
}
