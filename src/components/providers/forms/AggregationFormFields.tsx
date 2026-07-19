import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Plus, Trash2, Loader2, Layers, Download, Server } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  fetchModelsForConfig,
  showFetchModelsError,
} from "@/lib/api/model-fetch";
import type { ClaudeApiKeyField } from "@/types";
import {
  AGG_ROLE_SUPPORTS_1M,
  type AggRoleKey,
  type AggregationDraft,
} from "./hooks/useAggregationDraftState";

interface AggregationFormFieldsProps {
  draft: AggregationDraft;
}

const API_FORMAT_OPTIONS = [
  { value: "anthropic", labelKey: "providerForm.apiFormatAnthropic" },
  { value: "openai_chat", labelKey: "providerForm.apiFormatOpenAIChat" },
  {
    value: "openai_responses",
    labelKey: "providerForm.apiFormatOpenAIResponses",
  },
  { value: "gemini_native", labelKey: "providerForm.apiFormatGeminiNative" },
] as const;

const ROLE_ROWS: Array<{
  key: AggRoleKey;
  labelKey: string;
  fallback: string;
}> = [
  {
    key: "sonnet",
    labelKey: "providerForm.modelRoleSonnet",
    fallback: "Sonnet",
  },
  { key: "opus", labelKey: "providerForm.modelRoleOpus", fallback: "Opus" },
  { key: "fable", labelKey: "providerForm.modelRoleFable", fallback: "Fable" },
  { key: "haiku", labelKey: "providerForm.modelRoleHaiku", fallback: "Haiku" },
  {
    key: "subagent",
    labelKey: "providerForm.modelRoleSubagent",
    fallback: "Subagent",
  },
  {
    key: "default",
    labelKey: "aggregation.roleDefault",
    fallback: "默认兜底",
  },
];

/**
 * 「供应商聚合」表单字段：多条上游 + 一键获取模型 +（按 Claude 角色的）模型映射。
 */
export function AggregationFormFields({ draft }: AggregationFormFieldsProps) {
  const { t } = useTranslation();
  const {
    upstreams,
    roles,
    legacyRoutes,
    addUpstream,
    removeUpstream,
    updateUpstream,
    updateRole,
    removeLegacyRoute,
  } = draft;

  const [fetching, setFetching] = useState(false);
  const [fetchedByUpstream, setFetchedByUpstream] = useState<
    Record<string, string[]>
  >({});

  const configuredUpstreams = useMemo(
    () => upstreams.filter((u) => u.baseUrl.trim()),
    [upstreams],
  );

  const upstreamLabel = (id: string) => {
    const u = upstreams.find((x) => x.id === id);
    if (!u) return id;
    return (
      u.name.trim() ||
      u.baseUrl.trim() ||
      t("aggregation.unnamedUpstream", { defaultValue: "未命名上游" })
    );
  };

  const handleFetchAll = async () => {
    const targets = upstreams.filter((u) => u.baseUrl.trim());
    if (targets.length === 0) {
      toast.error(
        t("aggregation.needUpstream", {
          defaultValue: "请先添加至少一条带 URL 的上游",
        }),
      );
      return;
    }
    setFetching(true);
    const next: Record<string, string[]> = {};
    let anySuccess = false;
    for (const u of targets) {
      try {
        const models = await fetchModelsForConfig(
          u.baseUrl.trim(),
          u.apiKey.trim(),
          u.isFullUrl,
          undefined,
          undefined,
          u.apiKeyField,
        );
        next[u.id] = models.map((m) => m.id);
        anySuccess = true;
      } catch (err) {
        next[u.id] = [];
        showFetchModelsError(err, t, {
          hasApiKey: !!u.apiKey.trim(),
          hasBaseUrl: !!u.baseUrl.trim(),
          apiKeyOptional: true,
        });
      }
    }
    setFetchedByUpstream(next);
    setFetching(false);
    if (anySuccess) {
      const total = Object.values(next).reduce((s, a) => s + a.length, 0);
      toast.success(
        t("aggregation.fetchedCount", {
          count: total,
          defaultValue: `已获取 ${total} 个模型`,
        }),
        { closeButton: true },
      );
    }
  };

  return (
    <div className="space-y-6">
      {/* 说明 */}
      <div className="flex items-start gap-2 rounded-lg border border-indigo-500/30 bg-indigo-500/5 p-3">
        <Layers className="mt-0.5 h-4 w-4 flex-shrink-0 text-indigo-500" />
        <div className="space-y-1 text-xs text-muted-foreground">
          <p className="font-medium text-foreground">
            {t("aggregation.title", { defaultValue: "供应商聚合" })}
          </p>
          <p>
            {t("aggregation.intro", {
              defaultValue:
                "添加多条上游（各自的 URL / 密钥 / API 格式），一键获取全部模型后，在下方模型映射里把每个 Claude 角色绑定到某条上游的模型。等同于把单一供应商扩展为多供应商。",
            })}
          </p>
          <p className="text-amber-600 dark:text-amber-400">
            {t("aggregation.proxyHint", {
              defaultValue:
                "聚合供应商需要通过本地代理生效：请在「设置 → 路由」中开启 Claude 的代理接管。",
            })}
          </p>
        </div>
      </div>

      {/* 上游列表 */}
      <div className="space-y-3">
        <div className="flex items-center justify-between">
          <Label className="flex items-center gap-1.5 text-sm font-semibold">
            <Server className="h-4 w-4 text-muted-foreground" />
            {t("aggregation.upstreams", { defaultValue: "上游供应商" })}
          </Label>
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={addUpstream}
          >
            <Plus className="mr-1 h-3.5 w-3.5" />
            {t("aggregation.addUpstream", { defaultValue: "添加上游" })}
          </Button>
        </div>

        {upstreams.map((u, idx) => (
          <div
            key={u.id}
            className="space-y-2 rounded-lg border border-border bg-card/50 p-3"
          >
            <div className="flex items-center gap-2">
              <span className="text-xs font-medium text-muted-foreground">
                #{idx + 1}
              </span>
              <Input
                value={u.name}
                onChange={(e) => updateUpstream(u.id, { name: e.target.value })}
                placeholder={t("aggregation.upstreamNamePlaceholder", {
                  defaultValue: "上游名称（可选，如 OpenAI 转发）",
                })}
                className="h-8 flex-1"
              />
              <Button
                type="button"
                variant="ghost"
                size="icon"
                className="h-8 w-8 text-muted-foreground hover:text-destructive"
                onClick={() => removeUpstream(u.id)}
                disabled={upstreams.length <= 1}
                aria-label={t("common.delete", { defaultValue: "删除" })}
              >
                <Trash2 className="h-4 w-4" />
              </Button>
            </div>
            {/* 请求地址（整行，避免与下拉挤在一起） */}
            <Input
              value={u.baseUrl}
              onChange={(e) =>
                updateUpstream(u.id, { baseUrl: e.target.value })
              }
              placeholder={t("aggregation.baseUrlPlaceholder", {
                defaultValue: "请求地址，如 https://api.example.com",
              })}
              className="h-8 font-mono text-xs"
            />
            {/* API 格式 + 认证字段：各占一半、min-w-0 + 截断，防止长标签溢出 */}
            <div className="grid grid-cols-1 gap-2 sm:grid-cols-2">
              <Select
                value={u.apiFormat}
                onValueChange={(v) =>
                  updateUpstream(u.id, {
                    apiFormat:
                      v as (typeof API_FORMAT_OPTIONS)[number]["value"],
                  })
                }
              >
                <SelectTrigger
                  className="h-8 min-w-0 [&>span]:truncate"
                  aria-label={t("providerForm.apiFormat", {
                    defaultValue: "API 格式",
                  })}
                >
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {API_FORMAT_OPTIONS.map((o) => (
                    <SelectItem key={o.value} value={o.value}>
                      {t(o.labelKey)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              <Select
                value={u.apiKeyField}
                onValueChange={(value) =>
                  updateUpstream(u.id, {
                    apiKeyField: value as ClaudeApiKeyField,
                  })
                }
              >
                <SelectTrigger
                  className="h-8 min-w-0 [&>span]:truncate"
                  aria-label={t("providerForm.authField", {
                    defaultValue: "认证字段",
                  })}
                >
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="ANTHROPIC_AUTH_TOKEN">
                    {t("providerForm.authFieldAuthToken", {
                      defaultValue: "ANTHROPIC_AUTH_TOKEN（默认）",
                    })}
                  </SelectItem>
                  <SelectItem value="ANTHROPIC_API_KEY">
                    {t("providerForm.authFieldApiKey", {
                      defaultValue: "ANTHROPIC_API_KEY",
                    })}
                  </SelectItem>
                </SelectContent>
              </Select>
            </div>
            <Input
              type="password"
              value={u.apiKey}
              onChange={(e) => updateUpstream(u.id, { apiKey: e.target.value })}
              placeholder={t("aggregation.apiKeyPlaceholder", {
                defaultValue: "API Key（无鉴权可留空）",
              })}
              className="h-8 font-mono text-xs"
            />
            {fetchedByUpstream[u.id] && (
              <p className="text-xs text-muted-foreground">
                {t("aggregation.fetchedForUpstream", {
                  count: fetchedByUpstream[u.id].length,
                  defaultValue: `已获取 ${fetchedByUpstream[u.id].length} 个模型（下方映射输入框可自动补全）`,
                })}
              </p>
            )}
          </div>
        ))}

        <Button
          type="button"
          variant="secondary"
          size="sm"
          onClick={handleFetchAll}
          disabled={fetching}
          className="w-full"
        >
          {fetching ? (
            <Loader2 className="mr-1.5 h-4 w-4 animate-spin" />
          ) : (
            <Download className="mr-1.5 h-4 w-4" />
          )}
          {t("aggregation.fetchAll", {
            defaultValue: "一键获取全部上游的模型",
          })}
        </Button>
      </div>

      {/* 每条上游的模型建议（datalist，用于映射输入框自动补全） */}
      {Object.entries(fetchedByUpstream).map(([upstreamId, models]) => (
        <datalist key={upstreamId} id={`agg-models-${upstreamId}`}>
          {models.map((m) => (
            <option key={m} value={m} />
          ))}
        </datalist>
      ))}

      {/* 模型映射：按 Claude 角色（与单一供应商同构，左=角色，右=上游+模型） */}
      <div className="space-y-3">
        <Label className="text-sm font-semibold">
          {t("aggregation.modelRoutes", { defaultValue: "模型映射" })}
        </Label>
        <p className="text-xs text-muted-foreground">
          {t("aggregation.roleMappingHint", {
            defaultValue:
              "左侧是 Claude 角色，右侧选择由哪条上游、用哪个模型来承接。留空的角色不生效；建议至少配置「默认兜底」。",
          })}
        </p>

        {/* 表头 */}
        <div className="hidden grid-cols-[72px_1fr_1fr_auto] gap-2 px-1 text-xs font-medium text-muted-foreground sm:grid">
          <span>{t("aggregation.colRole", { defaultValue: "角色" })}</span>
          <span>{t("aggregation.colUpstream", { defaultValue: "上游" })}</span>
          <span>
            {t("aggregation.colModel", { defaultValue: "实际请求模型" })}
          </span>
          <span className="pr-1">1M</span>
        </div>

        <div className="space-y-2">
          {ROLE_ROWS.map(({ key, labelKey, fallback }) => {
            const role = roles[key];
            const supports1m = AGG_ROLE_SUPPORTS_1M[key];
            return (
              <div
                key={key}
                className="grid grid-cols-[72px_1fr_1fr_auto] items-center gap-2"
              >
                <span className="truncate text-sm font-medium text-muted-foreground">
                  {t(labelKey, { defaultValue: fallback })}
                </span>
                <Select
                  value={role.upstreamId || undefined}
                  onValueChange={(v) => updateRole(key, { upstreamId: v })}
                >
                  <SelectTrigger className="h-8 min-w-0 [&>span]:truncate">
                    <SelectValue
                      placeholder={t("aggregation.selectUpstream", {
                        defaultValue: "选择上游",
                      })}
                    />
                  </SelectTrigger>
                  <SelectContent>
                    {configuredUpstreams.map((u) => (
                      <SelectItem key={u.id} value={u.id}>
                        {upstreamLabel(u.id)}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
                <Input
                  value={role.model}
                  onChange={(e) => updateRole(key, { model: e.target.value })}
                  list={
                    role.upstreamId
                      ? `agg-models-${role.upstreamId}`
                      : undefined
                  }
                  placeholder={t("aggregation.modelPlaceholder", {
                    defaultValue: "实际请求模型名",
                  })}
                  className="h-8 font-mono text-xs"
                />
                {supports1m ? (
                  <label className="flex items-center justify-center pr-1">
                    <Checkbox
                      checked={role.supports1m}
                      onCheckedChange={(c) =>
                        updateRole(key, { supports1m: c === true })
                      }
                      aria-label="1M"
                    />
                  </label>
                ) : (
                  <span className="w-4 pr-1" />
                )}
              </div>
            );
          })}
        </div>
      </div>

      {legacyRoutes.length > 0 && (
        <div className="space-y-2 rounded-lg border border-amber-500/30 bg-amber-500/5 p-3">
          <Label className="text-sm font-semibold">
            {t("aggregation.legacyRoutesTitle", {
              count: legacyRoutes.length,
              defaultValue: `旧版模型路由（${legacyRoutes.length}）`,
            })}
          </Label>
          <p className="text-xs text-muted-foreground">
            {t("aggregation.legacyRoutesHint", {
              defaultValue:
                "这些路由来自旧版聚合配置，会继续在角色映射之后生效。请先完成新的角色映射，再按需删除旧路由。",
            })}
          </p>
          <div className="space-y-1.5">
            {legacyRoutes.map((route, index) => (
              <div
                key={`${route.model}-${route.upstreamId}-${index}`}
                className="flex items-center gap-2 rounded-md border bg-background/70 px-2 py-1.5 text-xs"
              >
                <code className="min-w-0 flex-1 truncate">{route.model}</code>
                <span className="text-muted-foreground">→</span>
                <span className="min-w-0 flex-1 truncate">
                  {upstreamLabel(route.upstreamId)}
                </span>
                {route.upstreamModel && (
                  <>
                    <span className="text-muted-foreground">→</span>
                    <code className="min-w-0 flex-1 truncate">
                      {route.upstreamModel}
                    </code>
                  </>
                )}
                <Button
                  type="button"
                  variant="ghost"
                  size="icon"
                  className="h-7 w-7 shrink-0 text-muted-foreground hover:text-destructive"
                  onClick={() => removeLegacyRoute(index)}
                  aria-label={t("common.delete", { defaultValue: "删除" })}
                >
                  <Trash2 className="h-3.5 w-3.5" />
                </Button>
              </div>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}
