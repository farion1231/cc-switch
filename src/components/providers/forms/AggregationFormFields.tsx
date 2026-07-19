import { useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Plus, Trash2, Loader2, Layers, Download, Server } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
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
import type { AggregationDraft } from "./hooks/useAggregationDraftState";

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

/**
 * 「供应商聚合」表单字段：多条上游 + 一键获取模型 + 模型→上游路由。
 */
export function AggregationFormFields({ draft }: AggregationFormFieldsProps) {
  const { t } = useTranslation();
  const {
    upstreams,
    routes,
    addUpstream,
    removeUpstream,
    updateUpstream,
    addRoute,
    removeRoute,
    updateRoute,
  } = draft;

  const [fetching, setFetching] = useState(false);
  // 每条上游拉取到的模型名（用于快速添加路由）
  const [fetchedByUpstream, setFetchedByUpstream] = useState<
    Record<string, string[]>
  >({});

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
        );
        next[u.id] = models.map((m) => m.id);
        anySuccess = true;
      } catch (err) {
        next[u.id] = [];
        showFetchModelsError(err, t, {
          hasApiKey: !!u.apiKey.trim(),
          hasBaseUrl: !!u.baseUrl.trim(),
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

  const routeExists = (model: string, upstreamId: string) =>
    routes.some((r) => r.model === model && r.upstreamId === upstreamId);

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
                "添加多条上游（各自的 URL / 密钥 / API 格式），一键获取全部模型后，把想用的模型分别绑定到上游。启用后作为一个供应商切换使用，代理会按模型名路由。",
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
            <div className="grid gap-2 sm:grid-cols-[1fr_200px]">
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
              <Select
                value={u.apiFormat}
                onValueChange={(v) =>
                  updateUpstream(u.id, {
                    apiFormat:
                      v as (typeof API_FORMAT_OPTIONS)[number]["value"],
                  })
                }
              >
                <SelectTrigger className="h-8">
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
            </div>
            <Input
              type="password"
              value={u.apiKey}
              onChange={(e) => updateUpstream(u.id, { apiKey: e.target.value })}
              placeholder={t("aggregation.apiKeyPlaceholder", {
                defaultValue: "API Key",
              })}
              className="h-8 font-mono text-xs"
            />
            {/* 已获取模型：点击快速加为路由 */}
            {fetchedByUpstream[u.id] && fetchedByUpstream[u.id].length > 0 && (
              <div className="flex flex-wrap gap-1.5 pt-1">
                {fetchedByUpstream[u.id].map((m) => {
                  const added = routeExists(m, u.id);
                  return (
                    <button
                      key={m}
                      type="button"
                      disabled={added}
                      onClick={() => addRoute({ model: m, upstreamId: u.id })}
                      className={`rounded-full border px-2 py-0.5 text-xs transition-colors ${
                        added
                          ? "cursor-default border-border bg-muted text-muted-foreground"
                          : "border-indigo-500/40 text-indigo-600 hover:bg-indigo-500/10 dark:text-indigo-400"
                      }`}
                      title={
                        added
                          ? t("aggregation.alreadyRouted", {
                              defaultValue: "已添加",
                            })
                          : t("aggregation.clickToRoute", {
                              defaultValue: "点击添加为路由",
                            })
                      }
                    >
                      {added ? "✓ " : "+ "}
                      {m}
                    </button>
                  );
                })}
              </div>
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

      {/* 模型路由 */}
      <div className="space-y-3">
        <div className="flex items-center justify-between">
          <Label className="text-sm font-semibold">
            {t("aggregation.modelRoutes", { defaultValue: "模型映射" })}
          </Label>
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={() => addRoute()}
          >
            <Plus className="mr-1 h-3.5 w-3.5" />
            {t("aggregation.addRoute", { defaultValue: "添加映射" })}
          </Button>
        </div>

        {routes.length === 0 ? (
          <p className="rounded-lg border border-dashed border-muted-foreground/40 p-4 text-center text-xs text-muted-foreground">
            {t("aggregation.noRoutes", {
              defaultValue:
                "暂无模型映射。先获取模型并点击添加，或手动添加「模型名 → 上游」。",
            })}
          </p>
        ) : (
          <div className="space-y-2">
            {routes.map((r, index) => (
              <div
                key={index}
                className="grid items-center gap-2 sm:grid-cols-[1fr_1fr_1fr_auto]"
              >
                <Input
                  value={r.model}
                  onChange={(e) =>
                    updateRoute(index, { model: e.target.value })
                  }
                  placeholder={t("aggregation.modelPlaceholder", {
                    defaultValue: "模型名 / 通配（如 gpt-*）",
                  })}
                  className="h-8 font-mono text-xs"
                />
                <Select
                  value={r.upstreamId}
                  onValueChange={(v) => updateRoute(index, { upstreamId: v })}
                >
                  <SelectTrigger className="h-8">
                    <SelectValue
                      placeholder={t("aggregation.selectUpstream", {
                        defaultValue: "选择上游",
                      })}
                    />
                  </SelectTrigger>
                  <SelectContent>
                    {upstreams
                      .filter((u) => u.baseUrl.trim())
                      .map((u) => (
                        <SelectItem key={u.id} value={u.id}>
                          {upstreamLabel(u.id)}
                        </SelectItem>
                      ))}
                  </SelectContent>
                </Select>
                <Input
                  value={r.upstreamModel}
                  onChange={(e) =>
                    updateRoute(index, { upstreamModel: e.target.value })
                  }
                  placeholder={t("aggregation.upstreamModelPlaceholder", {
                    defaultValue: "上游模型名（可选改写）",
                  })}
                  className="h-8 font-mono text-xs"
                />
                <Button
                  type="button"
                  variant="ghost"
                  size="icon"
                  className="h-8 w-8 text-muted-foreground hover:text-destructive"
                  onClick={() => removeRoute(index)}
                  aria-label={t("common.delete", { defaultValue: "删除" })}
                >
                  <Trash2 className="h-4 w-4" />
                </Button>
              </div>
            ))}
          </div>
        )}
        <p className="text-xs text-muted-foreground">
          {t("aggregation.matchHint", {
            defaultValue:
              "匹配规则：精确匹配优先，其次是最长前缀通配（如 gpt-*），最后是 * 兜底。",
          })}
        </p>
      </div>
    </div>
  );
}
