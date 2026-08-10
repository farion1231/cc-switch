import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { FormLabel } from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Plus, Trash2, ChevronRight, Download, Loader2 } from "lucide-react";
import { toast } from "sonner";
import { ApiKeySection, ModelDropdown } from "./shared";
import {
  fetchModelsForConfig,
  showFetchModelsError,
  type FetchedModel,
} from "@/lib/api/model-fetch";
import {
  PI_API_TYPES,
  type PiApiType,
  type PiModel,
} from "@/config/piProviderPresets";
import type { ProviderCategory } from "@/types";
import { cn } from "@/lib/utils";

interface PiFormFieldsProps {
  baseUrl: string;
  onBaseUrlChange: (url: string) => void;
  apiKey: string;
  onApiKeyChange: (key: string) => void;
  apiType: PiApiType;
  onApiTypeChange: (type: PiApiType) => void;
  models: PiModel[];
  onModelsChange: (models: PiModel[]) => void;
  category?: ProviderCategory;
  shouldShowApiKeyLink: boolean;
  websiteUrl: string;
  isPartner?: boolean;
  partnerPromotionKey?: string;
  /** The env key name for the base URL (e.g. ANTHROPIC_BASE_URL or OPENAI_BASE_URL) */
  baseUrlEnvKey?: string;
  /** The env key name for the API key (e.g. ANTHROPIC_API_KEY or OPENAI_API_KEY) */
  apiKeyEnvKey?: string;
}

/** Parse a numeric input string; empty => undefined, invalid => NaN sentinel (caller clamps). */
function parseOptionalInt(value: string): number | undefined {
  const t = value.trim();
  if (t === "") return undefined;
  const n = Number(t);
  return Number.isFinite(n) && n >= 0 ? Math.trunc(n) : undefined;
}

function parseOptionalFloat(value: string): number | undefined {
  const t = value.trim();
  if (t === "") return undefined;
  const n = Number(t);
  return Number.isFinite(n) && n >= 0 ? n : undefined;
}

export function PiFormFields({
  baseUrl,
  onBaseUrlChange,
  apiKey,
  onApiKeyChange,
  apiType,
  onApiTypeChange,
  models,
  onModelsChange,
  category,
  shouldShowApiKeyLink,
  websiteUrl,
  isPartner,
  partnerPromotionKey,
  baseUrlEnvKey = "ANTHROPIC_BASE_URL",
  apiKeyEnvKey = "ANTHROPIC_API_KEY",
}: PiFormFieldsProps) {
  const { t } = useTranslation();
  const modelKeysRef = useRef<string[]>(models.map(() => crypto.randomUUID()));

  // ── Fetch models ─────────────────────────────────────────────
  const [fetchedModels, setFetchedModels] = useState<FetchedModel[]>([]);
  const [isFetchingModels, setIsFetchingModels] = useState(false);

  const handleFetchModels = useCallback(() => {
    if (!baseUrl || !apiKey) {
      showFetchModelsError(null, t, {
        hasApiKey: !!apiKey,
        hasBaseUrl: !!baseUrl,
      });
      return;
    }
    setIsFetchingModels(true);
    fetchModelsForConfig(baseUrl, apiKey)
      .then((list) => {
        setFetchedModels(list);
        if (list.length === 0) {
          toast.info(t("providerForm.fetchModelsEmpty"));
          return;
        }
        toast.success(
          t("providerForm.fetchModelsSuccess", { count: list.length }),
        );
        // Merge fetched models into the list, dedup by id (keep existing entries).
        const existingIds = new Set(models.map((m) => m.id).filter(Boolean));
        const toAdd = list.filter((m) => m.id && !existingIds.has(m.id));
        if (toAdd.length > 0) {
          onModelsChange([
            ...models,
            ...toAdd.map((m) => ({ id: m.id, name: m.id })),
          ]);
        }
      })
      .catch((err) => {
        console.warn("[PiModelFetch] Failed:", err);
        showFetchModelsError(err, t);
      })
      .finally(() => setIsFetchingModels(false));
  }, [baseUrl, apiKey, models, onModelsChange, t]);

  // ── Model rows / expand state ────────────────────────────────
  const [expandedKeys, setExpandedKeys] = useState<Set<string>>(new Set());

  const toggleExpand = (key: string) => {
    setExpandedKeys((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  };

  // Keep keys array in sync with models length (both grow and shrink)
  useEffect(() => {
    while (modelKeysRef.current.length < models.length) {
      modelKeysRef.current.push(crypto.randomUUID());
    }
    if (modelKeysRef.current.length > models.length) {
      modelKeysRef.current.length = models.length;
    }
  }, [models.length]);

  const handleAddModel = () => {
    modelKeysRef.current.push(crypto.randomUUID());
    // Pi defaults reasoning to false when the field is absent; leave it unset
    // so the value is not written to models.json (Pi applies its default).
    onModelsChange([...models, { id: "", name: "" }]);
  };

  const handleRemoveModel = (index: number) => {
    const removedKey = modelKeysRef.current[index];
    modelKeysRef.current.splice(index, 1);
    onModelsChange(models.filter((_, i) => i !== index));
    if (removedKey) {
      setExpandedKeys((prev) => {
        const next = new Set(prev);
        next.delete(removedKey);
        return next;
      });
    }
  };

  const handleModelFieldChange = (
    index: number,
    field: keyof PiModel,
    value: string,
  ) => {
    const updated = models.map((m, i) =>
      i === index ? { ...m, [field]: value } : m,
    );
    onModelsChange(updated);
  };

  const handleModelNumberFieldChange = (
    index: number,
    field: "contextWindow" | "maxTokens",
    value: string,
  ) => {
    const parsed = parseOptionalInt(value);
    const updated = models.map((m, i) => {
      if (i !== index) return m;
      const next = { ...m };
      if (parsed === undefined) {
        delete next[field];
      } else {
        next[field] = parsed;
      }
      return next;
    });
    onModelsChange(updated);
  };

  const handleModelReasoningChange = (index: number, value: string) => {
    const updated = models.map((m, i) =>
      i === index ? { ...m, reasoning: value === "on" } : m,
    );
    onModelsChange(updated);
  };

  const handleModelCostChange = (
    index: number,
    field: "input" | "output" | "cacheRead" | "cacheWrite",
    value: string,
  ) => {
    const parsed = parseOptionalFloat(value);
    const updated = models.map((m, i) => {
      if (i !== index) return m;
      const cost = { ...(m.cost ?? {}) } as NonNullable<PiModel["cost"]>;
      if (parsed === undefined) {
        delete cost[field];
      } else {
        cost[field] = parsed;
      }
      const next: PiModel = { ...m };
      if (Object.keys(cost).length === 0) {
        delete next.cost;
      } else {
        next.cost = cost;
      }
      return next;
    });
    onModelsChange(updated);
  };

  return (
    <>
      {/* API protocol type selector */}
      <div className="space-y-2">
        <FormLabel htmlFor="pi-api-type">
          {t("pi.form.apiType", { defaultValue: "API 协议" })}
        </FormLabel>
        <Select
          value={apiType}
          onValueChange={(v) => onApiTypeChange(v as PiApiType)}
        >
          <SelectTrigger id="pi-api-type">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {PI_API_TYPES.map((item) => (
              <SelectItem key={item.value} value={item.value}>
                {item.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <p className="text-xs text-muted-foreground">
          {t("pi.form.apiTypeHint", {
            defaultValue:
              "供应商 API 协议类型。Anthropic 兼容选 anthropic-messages，OpenAI 兼容选 openai-completions。",
          })}
        </p>
      </div>

      <div className="space-y-2">
        <FormLabel htmlFor="pi-baseurl">
          {t("providerForm.apiEndpoint", { defaultValue: "API 端点" })}
          <span className="text-muted-foreground ml-1 text-xs font-normal">
            ({baseUrlEnvKey})
          </span>
        </FormLabel>
        <Input
          id="pi-baseurl"
          value={baseUrl}
          onChange={(e) => onBaseUrlChange(e.target.value)}
          placeholder="https://api.example.com/v1"
        />
        <p className="text-xs text-muted-foreground">
          {t("pi.form.baseUrlHint", {
            defaultValue: "Pi 的 API 端点地址，将写入环境变量 {{key}}。",
            key: baseUrlEnvKey,
          })}
        </p>
      </div>

      <ApiKeySection
        id="pi-apikey"
        label={`${t("providerForm.apiKey", { defaultValue: "API Key" })} (${apiKeyEnvKey})`}
        value={apiKey}
        onChange={onApiKeyChange}
        category={category}
        shouldShowLink={shouldShowApiKeyLink}
        websiteUrl={websiteUrl}
        isPartner={isPartner}
        partnerPromotionKey={partnerPromotionKey}
      />

      {/* Models Editor */}
      <div className="space-y-3">
        <div className="flex items-center justify-between">
          <FormLabel>
            {t("pi.form.models", { defaultValue: "模型列表" })}
          </FormLabel>
          <div className="flex gap-1">
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={handleFetchModels}
              disabled={isFetchingModels}
              className="h-7 gap-1"
            >
              {isFetchingModels ? (
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
              ) : (
                <Download className="h-3.5 w-3.5" />
              )}
              {t("providerForm.fetchModels")}
            </Button>
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={handleAddModel}
              className="h-7 gap-1"
            >
              <Plus className="h-3.5 w-3.5" />
              {t("pi.form.addModel", { defaultValue: "添加模型" })}
            </Button>
          </div>
        </div>
        <p className="text-xs text-muted-foreground">
          {t("pi.form.modelsHint", {
            defaultValue:
              "Pi 需要至少一个模型条目。id 为必填项（如 claude-sonnet-4-20250514），name 为可选显示名称。",
          })}
        </p>

        {models.length === 0 && (
          <p className="text-sm text-muted-foreground italic py-2">
            {t("pi.form.noModels", {
              defaultValue: '暂无模型，请点击"添加模型"按钮。',
            })}
          </p>
        )}

        {models.map((model, index) => {
          const rowKey = modelKeysRef.current[index] ?? `${index}`;
          const expanded = expandedKeys.has(rowKey);
          return (
            <div
              key={rowKey}
              className="border-border/50 space-y-2 rounded-lg border p-3"
            >
              <div className="flex items-center justify-between gap-2">
                <div className="flex items-center gap-2 min-w-0">
                  <button
                    type="button"
                    onClick={() => toggleExpand(rowKey)}
                    aria-label={t("opencode.toggleModelDetails", {
                      defaultValue: "Toggle model details",
                    })}
                    className="h-6 w-6 shrink-0 text-muted-foreground"
                  >
                    <ChevronRight
                      className={cn(
                        "h-4 w-4 transition-transform",
                        expanded && "rotate-90",
                      )}
                    />
                  </button>
                  <span className="bg-muted text-muted-foreground inline-flex items-center rounded px-1.5 py-0.5 text-xs font-medium">
                    {index === 0
                      ? t("pi.form.primaryModel", { defaultValue: "默认模型" })
                      : t("pi.form.fallbackModel", {
                          defaultValue: "备选模型",
                        })}
                  </span>
                </div>
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  onClick={() => handleRemoveModel(index)}
                  className="text-destructive hover:text-destructive h-7 w-7 p-0"
                >
                  <Trash2 className="h-3.5 w-3.5" />
                </Button>
              </div>

              <div className="grid grid-cols-2 gap-2">
                <div className="space-y-1">
                  <FormLabel className="text-xs">
                    {t("pi.form.modelId", { defaultValue: "模型 ID" })}
                    <span className="text-destructive ml-0.5">*</span>
                  </FormLabel>
                  <div className="flex gap-1">
                    <Input
                      value={model.id}
                      onChange={(e) =>
                        handleModelFieldChange(index, "id", e.target.value)
                      }
                      placeholder="claude-sonnet-4-20250514"
                      className="h-8 text-sm flex-1"
                    />
                    {fetchedModels.length > 0 && (
                      <ModelDropdown
                        models={fetchedModels}
                        onSelect={(id) =>
                          handleModelFieldChange(index, "id", id)
                        }
                      />
                    )}
                  </div>
                </div>
                <div className="space-y-1">
                  <FormLabel className="text-xs">
                    {t("pi.form.modelName", { defaultValue: "显示名称" })}
                  </FormLabel>
                  <Input
                    value={model.name ?? ""}
                    onChange={(e) =>
                      handleModelFieldChange(index, "name", e.target.value)
                    }
                    placeholder="Claude Sonnet 4"
                    className="h-8 text-sm"
                  />
                </div>
              </div>

              {/* Expanded model details */}
              {expanded && (
                <div className="space-y-3 border-t pt-3">
                  <div className="grid grid-cols-2 gap-2">
                    <div className="space-y-1">
                      <FormLabel className="text-xs">
                        {t("pi.form.contextWindow", {
                          defaultValue: "上下文窗口",
                        })}
                      </FormLabel>
                      <Input
                        type="number"
                        min={0}
                        step={1}
                        value={model.contextWindow ?? ""}
                        onChange={(e) =>
                          handleModelNumberFieldChange(
                            index,
                            "contextWindow",
                            e.target.value,
                          )
                        }
                        placeholder="200000"
                        className="h-8 text-sm"
                      />
                    </div>
                    <div className="space-y-1">
                      <FormLabel className="text-xs">
                        {t("pi.form.maxTokens", {
                          defaultValue: "最大输出 Tokens",
                        })}
                      </FormLabel>
                      <Input
                        type="number"
                        min={0}
                        step={1}
                        value={model.maxTokens ?? ""}
                        onChange={(e) =>
                          handleModelNumberFieldChange(
                            index,
                            "maxTokens",
                            e.target.value,
                          )
                        }
                        placeholder="64000"
                        className="h-8 text-sm"
                      />
                    </div>
                  </div>

                  <div className="space-y-1">
                    <FormLabel className="text-xs">
                      {t("pi.form.reasoning", { defaultValue: "推理模式" })}
                    </FormLabel>
                    <Select
                      value={model.reasoning === true ? "on" : "off"}
                      onValueChange={(v) =>
                        handleModelReasoningChange(index, v)
                      }
                    >
                      <SelectTrigger className="h-8 text-sm">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="off">
                          {t("pi.form.reasoningOff", { defaultValue: "关闭" })}
                        </SelectItem>
                        <SelectItem value="on">
                          {t("pi.form.reasoningOn", { defaultValue: "启用" })}
                        </SelectItem>
                      </SelectContent>
                    </Select>
                  </div>

                  <p className="text-xs font-medium text-muted-foreground">
                    {t("pi.form.costSection", {
                      defaultValue: "价格（$/M tokens，可选）",
                    })}
                  </p>
                  <div className="grid grid-cols-2 gap-2">
                    <div className="space-y-1">
                      <FormLabel className="text-xs">
                        {t("pi.form.inputCost", {
                          defaultValue: "输入价格 ($/M tokens)",
                        })}
                      </FormLabel>
                      <Input
                        type="number"
                        min={0}
                        step="any"
                        value={model.cost?.input ?? ""}
                        onChange={(e) =>
                          handleModelCostChange(index, "input", e.target.value)
                        }
                        placeholder="3"
                        className="h-8 text-sm"
                      />
                    </div>
                    <div className="space-y-1">
                      <FormLabel className="text-xs">
                        {t("pi.form.outputCost", {
                          defaultValue: "输出价格 ($/M tokens)",
                        })}
                      </FormLabel>
                      <Input
                        type="number"
                        min={0}
                        step="any"
                        value={model.cost?.output ?? ""}
                        onChange={(e) =>
                          handleModelCostChange(index, "output", e.target.value)
                        }
                        placeholder="15"
                        className="h-8 text-sm"
                      />
                    </div>
                    <div className="space-y-1">
                      <FormLabel className="text-xs">
                        {t("pi.form.cacheReadCost", {
                          defaultValue: "缓存读取价格 ($/M tokens)",
                        })}
                      </FormLabel>
                      <Input
                        type="number"
                        min={0}
                        step="any"
                        value={model.cost?.cacheRead ?? ""}
                        onChange={(e) =>
                          handleModelCostChange(
                            index,
                            "cacheRead",
                            e.target.value,
                          )
                        }
                        placeholder="0.3"
                        className="h-8 text-sm"
                      />
                    </div>
                    <div className="space-y-1">
                      <FormLabel className="text-xs">
                        {t("pi.form.cacheWriteCost", {
                          defaultValue: "缓存写入价格 ($/M tokens)",
                        })}
                      </FormLabel>
                      <Input
                        type="number"
                        min={0}
                        step="any"
                        value={model.cost?.cacheWrite ?? ""}
                        onChange={(e) =>
                          handleModelCostChange(
                            index,
                            "cacheWrite",
                            e.target.value,
                          )
                        }
                        placeholder="3.75"
                        className="h-8 text-sm"
                      />
                    </div>
                  </div>
                </div>
              )}
            </div>
          );
        })}
      </div>
    </>
  );
}
