import { useState, useEffect, useCallback } from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import { FormLabel } from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import { ImeSafeInput } from "@/components/ui/ime-safe-input";
import { Button } from "@/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { Textarea } from "@/components/ui/textarea";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import { toast } from "sonner";
import {
  Download,
  Plus,
  Trash2,
  Check,
  ChevronRight,
  Loader2,
  Sparkles,
  X,
} from "lucide-react";
import { ApiKeySection, ModelDropdown } from "./shared";
import {
  fetchModelsForConfig,
  showFetchModelsError,
  type FetchedModel,
} from "@/lib/api/model-fetch";
import {
  fetchModelsDevPricing,
  MODELS_DEV_QUERY_KEY,
  MODELS_DEV_STALE_TIME_MS,
} from "@/lib/modelsDevPricing";
import {
  ALL_EFFORT_LEVELS,
  applyModelsDevCapabilities,
  buildVariantForEffort,
  findModelsDevEntry,
  nextNumberedKey,
  type ModelsDevVariantsStyle,
} from "@/lib/opencodeModelCapabilities";
import { useSaveSettingsMutation, useSettingsQuery } from "@/lib/query";
import { opencodeNpmPackages } from "@/config/opencodeProviderPresets";
import { cn } from "@/lib/utils";
import {
  getModelExtraFields,
  isKnownModelKey,
  OPENCODE_EXTRA_OPTION_DRAFT_PREFIX,
} from "./helpers/opencodeFormUtils";
import { RequestHeadersEditor } from "./RequestHeadersEditor";
import type { ProviderCategory, OpenCodeModel } from "@/types";

/**
 * Model ID input with local state to prevent focus loss.
 * The key prop issue: when Model ID changes, React sees it as a new element
 * and unmounts/remounts the input, losing focus. Using local state + onBlur
 * keeps the key stable during editing.
 */
function ModelIdInput({
  modelId,
  onChange,
  placeholder,
}: {
  modelId: string;
  onChange: (newId: string) => void;
  placeholder?: string;
}) {
  const [localValue, setLocalValue] = useState(modelId);

  // Sync when external modelId changes (e.g., undo operation)
  useEffect(() => {
    setLocalValue(modelId);
  }, [modelId]);

  return (
    <ImeSafeInput
      value={localValue}
      onValueChange={setLocalValue}
      onBlur={(event) => {
        const nextValue = event.currentTarget.value;
        if (nextValue !== modelId && nextValue.trim()) {
          onChange(nextValue);
        }
      }}
      placeholder={placeholder}
      className="flex-1"
    />
  );
}

/**
 * Extra option key input with local state to prevent focus loss.
 * Same pattern as ModelIdInput - use local state during editing,
 * only commit changes on blur.
 */
function ExtraOptionKeyInput({
  optionKey,
  onChange,
  placeholder,
  placeholderPrefixes = [OPENCODE_EXTRA_OPTION_DRAFT_PREFIX],
}: {
  optionKey: string;
  onChange: (newKey: string) => boolean | void;
  placeholder?: string;
  placeholderPrefixes?: string[];
}) {
  const isPlaceholderKey = placeholderPrefixes.some((prefix) =>
    optionKey.startsWith(prefix),
  );
  const displayValue = isPlaceholderKey ? "" : optionKey;
  const [localValue, setLocalValue] = useState(displayValue);

  // Sync when external key changes
  useEffect(() => {
    setLocalValue(isPlaceholderKey ? "" : optionKey);
  }, [isPlaceholderKey, optionKey]);

  return (
    <ImeSafeInput
      value={localValue}
      onValueChange={setLocalValue}
      onBlur={(event) => {
        const trimmed = event.currentTarget.value.trim();
        if (trimmed && trimmed !== optionKey) {
          const accepted = onChange(trimmed);
          if (accepted === false) {
            setLocalValue(displayValue);
          }
        }
      }}
      placeholder={placeholder}
      className="flex-1"
    />
  );
}

/**
 * Model option key input with local state to prevent focus loss.
 * Reuses the same pattern as ExtraOptionKeyInput.
 */
function ModelOptionKeyInput({
  optionKey,
  onChange,
  placeholder,
}: {
  optionKey: string;
  onChange: (newKey: string) => void;
  placeholder?: string;
}) {
  const displayValue = optionKey.startsWith("option-") ? "" : optionKey;
  const [localValue, setLocalValue] = useState(displayValue);

  useEffect(() => {
    setLocalValue(optionKey.startsWith("option-") ? "" : optionKey);
  }, [optionKey]);

  return (
    <ImeSafeInput
      value={localValue}
      onValueChange={setLocalValue}
      onBlur={(event) => {
        const trimmed = event.currentTarget.value.trim();
        if (trimmed && trimmed !== optionKey) {
          onChange(trimmed);
        }
        // Reset to prop value: if parent accepted the rename, useEffect
        // will update localValue when the new optionKey prop arrives;
        // if parent rejected, this restores the correct display.
        setLocalValue(optionKey.startsWith("option-") ? "" : optionKey);
      }}
      placeholder={placeholder}
      className="flex-1"
    />
  );
}

/** opencode 模型的图片输入能力 = modalities.input 数组中包含 "image"。 */
function supportsImageInput(model: OpenCodeModel): boolean {
  const modalities = model.modalities as { input?: unknown } | undefined;
  return (
    Array.isArray(modalities?.input) &&
    (modalities.input as unknown[]).includes("image")
  );
}

/** 从档位参数体中提取人类可读的强度摘要（reasoningEffort / thinkingLevel / budget 等）。 */
function variantSummary(value: Record<string, unknown>): string | null {
  if (!value || typeof value !== "object") return null;
  if (value.disabled === true) return "disabled";
  const effort =
    value.reasoningEffort ?? value.effort ?? value.reasoning_effort;
  if (typeof effort === "string") return effort;
  const thinkingConfig = value.thinkingConfig as
    | { thinkingLevel?: unknown; thinkingBudget?: unknown }
    | undefined;
  if (typeof thinkingConfig?.thinkingLevel === "string") {
    return thinkingConfig.thinkingLevel;
  }
  if (typeof thinkingConfig?.thinkingBudget === "number") {
    return String(thinkingConfig.thinkingBudget);
  }
  const reasoningConfig = value.reasoningConfig as
    | { maxReasoningEffort?: unknown; budgetTokens?: unknown }
    | undefined;
  if (typeof reasoningConfig?.maxReasoningEffort === "string") {
    return String(reasoningConfig.maxReasoningEffort);
  }
  if (typeof reasoningConfig?.budgetTokens === "number") {
    return String(reasoningConfig.budgetTokens);
  }
  const thinking = value.thinking as { budgetTokens?: unknown } | undefined;
  if (typeof thinking?.budgetTokens === "number") {
    return String(thinking.budgetTokens);
  }
  return null;
}

/** 单枚档位胶囊：点击弹出 Popover 编辑参数体 JSON，X 移除档位。 */
function VariantEditorPopover({
  variantKey,
  value,
  onSave,
  onRemove,
}: {
  variantKey: string;
  value: Record<string, unknown>;
  onSave: (key: string, value: Record<string, unknown>) => void;
  onRemove: (key: string) => void;
}) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [draft, setDraft] = useState(() => JSON.stringify(value, null, 2));

  let parsed: Record<string, unknown> | null = null;
  try {
    const parsedDraft: unknown = JSON.parse(draft);
    if (
      parsedDraft &&
      typeof parsedDraft === "object" &&
      !Array.isArray(parsedDraft)
    ) {
      parsed = parsedDraft as Record<string, unknown>;
    }
  } catch {
    parsed = null;
  }

  const summary = variantSummary(value);

  return (
    <div className="flex items-center gap-0.5">
      <Popover
        open={open}
        onOpenChange={(next) => {
          setOpen(next);
          if (next) setDraft(JSON.stringify(value, null, 2));
        }}
      >
        <PopoverTrigger asChild>
          <button
            type="button"
            className="inline-flex h-6 max-w-56 items-center gap-1.5 rounded-full border border-border bg-transparent px-2.5 text-xs transition-colors hover:bg-accent"
          >
            <span className="font-medium">{variantKey}</span>
            {summary && (
              <span className="truncate text-muted-foreground">{summary}</span>
            )}
          </button>
        </PopoverTrigger>
        <PopoverContent align="start" className="w-80 space-y-2">
          <p className="text-xs font-medium">{variantKey}</p>
          <Textarea
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            rows={6}
            className="min-h-24 font-mono text-xs"
            spellCheck={false}
          />
          {!parsed && (
            <p className="text-xs text-destructive">
              {t("opencode.thinkingLevelJsonInvalid", {
                defaultValue: "Payload must be a valid JSON object",
              })}
            </p>
          )}
          <div className="flex justify-end">
            <Button
              type="button"
              size="sm"
              className="h-7 gap-1"
              disabled={!parsed}
              onClick={() => {
                if (!parsed) return;
                onSave(variantKey, parsed);
                setOpen(false);
              }}
            >
              <Check className="h-3.5 w-3.5" />
              {t("common.save", { defaultValue: "保存" })}
            </Button>
          </div>
        </PopoverContent>
      </Popover>
      <button
        type="button"
        onClick={() => onRemove(variantKey)}
        aria-label={t("opencode.removeThinkingLevel", {
          defaultValue: "Remove level {{key}}",
          key: variantKey,
        })}
        className="inline-flex h-5 w-5 items-center justify-center rounded-full text-muted-foreground transition-colors hover:bg-accent hover:text-destructive"
      >
        <X className="h-3 w-3" />
      </button>
    </div>
  );
}

/** 思考档位编辑区：variants 档位胶囊列表 + 添加档位下拉（按 SDK 自动生成参数体）。 */
function ThinkingLevelsEditor({
  modelId,
  npm,
  outputLimit,
  variants,
  onChange,
  style,
}: {
  modelId: string;
  npm: string;
  outputLimit?: number;
  variants: Record<string, Record<string, unknown>>;
  onChange: (
    variants: Record<string, Record<string, unknown>> | undefined,
  ) => void;
  style: ModelsDevVariantsStyle;
}) {
  const { t } = useTranslation();
  const [expanded, setExpanded] = useState(false);

  const entries = Object.entries(variants);
  const existingEfforts = new Set(
    entries.map(([vKey]) => /^(\d{1,2})-(.+)$/.exec(vKey)?.[2] ?? vKey),
  );
  // 全量档位供手动添加，隐藏已有档位（含 numbered 键解析出的 effort 名）
  const availableEfforts = ALL_EFFORT_LEVELS.filter(
    (effort) => !existingEfforts.has(effort),
  );

  const handleAddVariant = (effort: string) => {
    const payload = buildVariantForEffort(npm, modelId, effort, outputLimit);
    if (!payload) return;
    const nextKey =
      style === "numbered" ? nextNumberedKey(variants, effort) : effort;
    onChange({ ...variants, [nextKey]: payload });
  };

  const handleSaveVariant = (
    variantKey: string,
    value: Record<string, unknown>,
  ) => {
    onChange({ ...variants, [variantKey]: value });
  };

  const handleRemoveVariant = (variantKey: string) => {
    const next = { ...variants };
    delete next[variantKey];
    onChange(Object.keys(next).length > 0 ? next : undefined);
  };

  return (
    <div className="space-y-2">
      <div className="flex items-center justify-between">
        <button
          type="button"
          onClick={() => setExpanded((prev) => !prev)}
          className="inline-flex items-center gap-1 text-xs font-medium text-muted-foreground transition-colors hover:text-foreground"
        >
          <ChevronRight
            className={cn(
              "h-3.5 w-3.5 transition-transform",
              expanded && "rotate-90",
            )}
          />
          {t("opencode.thinkingLevels", { defaultValue: "Thinking levels" })}
        </button>
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button
              type="button"
              variant="ghost"
              size="sm"
              className="h-6 gap-1 px-2"
            >
              <Plus className="h-3 w-3" />
              {t("opencode.addThinkingLevel", { defaultValue: "Add level" })}
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent
            align="end"
            side="left"
            className="max-h-64 overflow-y-auto z-[200]"
          >
            {availableEfforts.length > 0 ? (
              availableEfforts.map((effort) => (
                <DropdownMenuItem
                  key={effort}
                  onSelect={() => handleAddVariant(effort)}
                >
                  {effort}
                </DropdownMenuItem>
              ))
            ) : (
              <p className="px-2 py-1.5 text-xs text-muted-foreground">
                {t("opencode.noAvailableEfforts", {
                  defaultValue: "No levels available for this SDK",
                })}
              </p>
            )}
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
      {expanded && (
        <div className="flex flex-wrap items-center gap-1.5">
          {entries.length > 0 ? (
            entries.map(([vKey, vValue]) => (
              <VariantEditorPopover
                key={vKey}
                variantKey={vKey}
                value={vValue}
                onSave={handleSaveVariant}
                onRemove={handleRemoveVariant}
              />
            ))
          ) : (
            <p className="text-xs text-muted-foreground py-1">
              {t("opencode.noThinkingLevels", {
                defaultValue:
                  "No thinking levels yet. Add one or use auto-fill",
              })}
            </p>
          )}
        </div>
      )}
    </div>
  );
}

interface OpenCodeFormFieldsProps {
  // NPM Package
  npm: string;
  onNpmChange: (value: string) => void;
  // API Key
  apiKey: string;
  onApiKeyChange: (value: string) => void;
  category?: ProviderCategory;
  shouldShowApiKeyLink: boolean;
  websiteUrl: string;
  isPartner?: boolean;
  partnerPromotionKey?: string;

  // Base URL
  baseUrl: string;
  onBaseUrlChange: (value: string) => void;

  // Headers
  headers: Record<string, string>;
  onHeadersChange: (headers: Record<string, string>) => void;

  // Models
  models: Record<string, OpenCodeModel>;
  onModelsChange: (models: Record<string, OpenCodeModel>) => void;

  // Extra Options
  extraOptions: Record<string, string>;
  onExtraOptionsChange: (options: Record<string, string>) => void;
}

export function OpenCodeFormFields({
  npm,
  onNpmChange,
  apiKey,
  onApiKeyChange,
  category,
  shouldShowApiKeyLink,
  websiteUrl,
  isPartner,
  partnerPromotionKey,
  baseUrl,
  onBaseUrlChange,
  headers,
  onHeadersChange,
  models,
  onModelsChange,
  extraOptions,
  onExtraOptionsChange,
}: OpenCodeFormFieldsProps) {
  const { t } = useTranslation();

  const [fetchedModels, setFetchedModels] = useState<FetchedModel[]>([]);
  const [isFetchingModels, setIsFetchingModels] = useState(false);

  const { refetch: refetchModelsDevCatalog } = useQuery({
    queryKey: MODELS_DEV_QUERY_KEY,
    queryFn: fetchModelsDevPricing,
    enabled: false,
    staleTime: MODELS_DEV_STALE_TIME_MS,
    retry: 1,
  });
  const [isFetchingCapabilities, setIsFetchingCapabilities] = useState(false);

  const { data: settingsData } = useSettingsQuery();
  const saveSettingsMutation = useSaveSettingsMutation();
  const variantsStyle: ModelsDevVariantsStyle =
    settingsData?.modelsDevVariantsStyle === "numbered" ? "numbered" : "plain";

  // 从 models.dev 拉取官方模型能力（limit/cost/modalities/variants）批量填充全部模型行
  const handleFillFromModelsDev = useCallback(
    async (styleOverride?: ModelsDevVariantsStyle) => {
      if (Object.keys(models).length === 0) {
        toast.info(t("opencode.fillCapabilitiesNoModels"));
        return;
      }
      const style = styleOverride ?? variantsStyle;
      setIsFetchingCapabilities(true);
      try {
        const { data: catalog } = await refetchModelsDevCatalog();
        if (!catalog) throw new Error("models.dev catalog unavailable");
        const nextModels: Record<string, OpenCodeModel> = {};
        const missing: string[] = [];
        const details: string[] = [];
        let filled = 0;
        for (const [key, model] of Object.entries(models)) {
          const hit = findModelsDevEntry(catalog, key);
          if (!hit) {
            missing.push(key);
            nextModels[key] = model;
            continue;
          }
          const { model: merged, summary } = applyModelsDevCapabilities(
            model,
            npm,
            key,
            hit.entry,
            { style },
          );
          nextModels[key] = merged;
          filled += 1;
          details.push(
            `${key} ← ${hit.providerId}` +
              (summary.contextLimit ? ` · ctx ${summary.contextLimit}` : "") +
              (summary.efforts.length
                ? ` · ${summary.efforts.join("/")}`
                : "") +
              (summary.suppressed.length
                ? ` · disabled: ${summary.suppressed.join("/")}`
                : ""),
          );
        }
        onModelsChange(nextModels);
        if (filled === 0) {
          toast.warning(t("opencode.fillCapabilitiesNone"), {
            description: missing.join(", "),
          });
        } else if (missing.length > 0) {
          toast.warning(
            t("opencode.fillCapabilitiesPartial", {
              count: filled,
              missing: missing.join(", "),
            }),
          );
        } else {
          toast.success(
            t("opencode.fillCapabilitiesSuccess", { count: filled }),
            {
              description: details.join("\n"),
            },
          );
        }
      } catch (err) {
        console.warn("[ModelsDevCapabilities] Failed:", err);
        toast.error(t("opencode.fillCapabilitiesFailed"));
      } finally {
        setIsFetchingCapabilities(false);
      }
    },
    [models, npm, onModelsChange, refetchModelsDevCatalog, t, variantsStyle],
  );

  // 切换思考档位名称偏好：仅保存设置（settings 持久化），填充按钮按当前偏好执行
  const handleVariantsStyleChange = useCallback(
    (style: ModelsDevVariantsStyle) => {
      if (!settingsData) return;
      void saveSettingsMutation
        .mutateAsync({ ...settingsData, modelsDevVariantsStyle: style })
        .catch((err) =>
          console.warn("[ModelsDevCapabilities] Failed to save style:", err),
        );
    },
    [settingsData, saveSettingsMutation],
  );

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
      .then((models) => {
        setFetchedModels(models);
        if (models.length === 0) {
          toast.info(t("providerForm.fetchModelsEmpty"));
        } else {
          toast.success(
            t("providerForm.fetchModelsSuccess", { count: models.length }),
          );
        }
      })
      .catch((err) => {
        console.warn("[ModelFetch] Failed:", err);
        showFetchModelsError(err, t);
      })
      .finally(() => setIsFetchingModels(false));
  }, [baseUrl, apiKey, t]);

  // Track which models have expanded options panel
  const [expandedModels, setExpandedModels] = useState<Set<string>>(new Set());

  // 模型属性（extra fields）KV 列表默认收起，点箭头展开
  const [extraFieldsExpanded, setExtraFieldsExpanded] = useState(false);

  // Toggle model expand state
  const toggleModelExpand = (key: string) => {
    setExpandedModels((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  };

  // Add a new model entry
  const handleAddModel = () => {
    const newKey = `model-${Date.now()}`;
    onModelsChange({
      ...models,
      [newKey]: { name: "" },
    });
  };

  // Remove a model entry
  const handleRemoveModel = (key: string) => {
    const newModels = { ...models };
    delete newModels[key];
    onModelsChange(newModels);
    // Also remove from expanded set
    setExpandedModels((prev) => {
      const next = new Set(prev);
      next.delete(key);
      return next;
    });
  };

  // Update model ID (key)
  const handleModelIdChange = (oldKey: string, newKey: string) => {
    if (oldKey === newKey || !newKey.trim()) return;
    const newModels: Record<string, OpenCodeModel> = {};
    for (const [k, v] of Object.entries(models)) {
      if (k === oldKey) {
        newModels[newKey] = v;
      } else {
        newModels[k] = v;
      }
    }
    onModelsChange(newModels);
    // Update expanded set if this model was expanded
    if (expandedModels.has(oldKey)) {
      setExpandedModels((prev) => {
        const next = new Set(prev);
        next.delete(oldKey);
        next.add(newKey);
        return next;
      });
    }
  };

  // Update model name
  const handleModelNameChange = (key: string, name: string) => {
    onModelsChange({
      ...models,
      [key]: { ...models[key], name },
    });
  };

  // Toggle "supports extended thinking" (model.reasoning)
  const handleModelReasoningChange = (modelKey: string, checked: boolean) => {
    onModelsChange({
      ...models,
      [modelKey]: { ...models[modelKey], reasoning: checked },
    });
  };

  // Toggle image input: add/remove "image" in model.modalities.input
  const handleModelImageInputChange = (modelKey: string, checked: boolean) => {
    const model = models[modelKey];
    const current = (
      model.modalities && typeof model.modalities === "object"
        ? model.modalities
        : {}
    ) as Record<string, unknown>;
    const currentInput = Array.isArray(current.input)
      ? (current.input as unknown[]).filter(
          (item): item is string => typeof item === "string",
        )
      : ["text"];
    const nextInput = checked
      ? currentInput.includes("image")
        ? currentInput
        : [...currentInput, "image"]
      : currentInput.filter((item) => item !== "image");
    onModelsChange({
      ...models,
      [modelKey]: {
        ...model,
        modalities: { ...current, input: nextInput },
      },
    });
  };

  // Update thinking-level variants of a model
  const handleModelVariantsChange = (
    modelKey: string,
    nextVariants: Record<string, Record<string, unknown>> | undefined,
  ) => {
    const nextModel = { ...models[modelKey] };
    if (nextVariants) {
      nextModel.variants = nextVariants;
    } else {
      delete nextModel.variants;
    }
    onModelsChange({ ...models, [modelKey]: nextModel });
  };

  const handleModelLimitChange = (
    modelKey: string,
    limitKey: "context" | "output",
    value: string,
  ) => {
    const model = models[modelKey];
    const nextLimit = { ...(model.limit || {}) };
    const trimmedValue = value.trim();

    if (trimmedValue === "") {
      delete nextLimit[limitKey];
    } else {
      const parsed = Number(trimmedValue);
      if (!Number.isFinite(parsed) || parsed < 0) return;
      nextLimit[limitKey] = Math.trunc(parsed);
    }

    const nextModel = { ...model };
    if (Object.keys(nextLimit).length > 0) {
      nextModel.limit = nextLimit;
    } else {
      delete nextModel.limit;
    }

    onModelsChange({
      ...models,
      [modelKey]: nextModel,
    });
  };

  // Model options handlers
  const handleAddModelOption = (modelKey: string) => {
    const model = models[modelKey];
    const newOptionKey = `option-${Date.now()}`;
    onModelsChange({
      ...models,
      [modelKey]: {
        ...model,
        options: { ...model.options, [newOptionKey]: "" },
      },
    });
  };

  const handleRemoveModelOption = (modelKey: string, optionKey: string) => {
    const model = models[modelKey];
    const newOptions = { ...model.options };
    delete newOptions[optionKey];
    onModelsChange({
      ...models,
      [modelKey]: {
        ...model,
        options: Object.keys(newOptions).length > 0 ? newOptions : undefined,
      },
    });
  };

  const handleModelOptionKeyChange = (
    modelKey: string,
    oldKey: string,
    newKey: string,
  ) => {
    if (!newKey.trim() || oldKey === newKey) return;
    const model = models[modelKey];
    const newOptions: Record<string, unknown> = {};
    for (const [k, v] of Object.entries(model.options || {})) {
      if (k === oldKey) newOptions[newKey] = v;
      else newOptions[k] = v;
    }
    onModelsChange({
      ...models,
      [modelKey]: { ...model, options: newOptions },
    });
  };

  const handleModelOptionValueChange = (
    modelKey: string,
    optionKey: string,
    value: string,
  ) => {
    const model = models[modelKey];
    let parsedValue: unknown;
    try {
      parsedValue = JSON.parse(value);
    } catch {
      parsedValue = value;
    }
    onModelsChange({
      ...models,
      [modelKey]: {
        ...model,
        options: { ...model.options, [optionKey]: parsedValue },
      },
    });
  };

  // Model extra field handlers (top-level properties like variants, cost)
  const handleAddModelExtraField = (modelKey: string) => {
    const model = models[modelKey];
    const newFieldKey = `option-${Date.now()}`;
    onModelsChange({
      ...models,
      [modelKey]: { ...model, [newFieldKey]: "" },
    });
  };

  const handleRemoveModelExtraField = (modelKey: string, fieldKey: string) => {
    const model = models[modelKey];
    const newModel = { ...model };
    delete newModel[fieldKey];
    onModelsChange({
      ...models,
      [modelKey]: newModel,
    });
  };

  const handleModelExtraFieldKeyChange = (
    modelKey: string,
    oldKey: string,
    newKey: string,
  ) => {
    if (!newKey.trim() || oldKey === newKey) return;
    const model = models[modelKey];
    // Reject reserved keys and duplicate extra field names
    if (isKnownModelKey(newKey) || (newKey !== oldKey && newKey in model))
      return;
    const newModel: Record<string, unknown> = {};
    for (const [k, v] of Object.entries(model)) {
      if (k === oldKey) newModel[newKey] = v;
      else newModel[k] = v;
    }
    onModelsChange({
      ...models,
      [modelKey]: newModel as OpenCodeModel,
    });
  };

  const handleModelExtraFieldValueChange = (
    modelKey: string,
    fieldKey: string,
    value: string,
  ) => {
    const model = models[modelKey];
    let parsedValue: unknown;
    try {
      parsedValue = JSON.parse(value);
    } catch {
      parsedValue = value;
    }
    onModelsChange({
      ...models,
      [modelKey]: { ...model, [fieldKey]: parsedValue },
    });
  };

  // Extra Options handlers
  const handleAddExtraOption = () => {
    const newKey = `${OPENCODE_EXTRA_OPTION_DRAFT_PREFIX}${Date.now()}`;
    onExtraOptionsChange({
      ...extraOptions,
      [newKey]: "",
    });
  };

  const handleRemoveExtraOption = (key: string) => {
    const newOptions = { ...extraOptions };
    delete newOptions[key];
    onExtraOptionsChange(newOptions);
  };

  const handleExtraOptionKeyChange = (oldKey: string, newKey: string) => {
    if (oldKey === newKey) return;
    const newOptions: Record<string, string> = {};
    for (const [k, v] of Object.entries(extraOptions)) {
      if (k === oldKey) {
        newOptions[newKey.trim() || oldKey] = v;
      } else {
        newOptions[k] = v;
      }
    }
    onExtraOptionsChange(newOptions);
  };

  const handleExtraOptionValueChange = (key: string, value: string) => {
    onExtraOptionsChange({
      ...extraOptions,
      [key]: value,
    });
  };

  return (
    <>
      {/* NPM Package Selector */}
      <div className="space-y-2">
        <FormLabel htmlFor="opencode-npm">
          {t("opencode.npmPackage", {
            defaultValue: "接口格式",
          })}
        </FormLabel>
        <Select value={npm} onValueChange={onNpmChange}>
          <SelectTrigger id="opencode-npm">
            <SelectValue
              placeholder={t("opencode.selectPackage", {
                defaultValue: "Select a package",
              })}
            />
          </SelectTrigger>
          <SelectContent>
            {opencodeNpmPackages.map((pkg) => (
              <SelectItem key={pkg.value} value={pkg.value}>
                {pkg.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <p className="text-xs text-muted-foreground">
          {t("opencode.npmPackageHint", {
            defaultValue:
              "Select the AI SDK package that matches your provider.",
          })}
        </p>
      </div>

      {/* API Key */}
      <ApiKeySection
        value={apiKey}
        onChange={onApiKeyChange}
        category={category}
        shouldShowLink={shouldShowApiKeyLink}
        websiteUrl={websiteUrl}
        isPartner={isPartner}
        partnerPromotionKey={partnerPromotionKey}
      />

      {/* Base URL */}
      <div className="space-y-2">
        <FormLabel htmlFor="opencode-baseurl">
          {t("opencode.baseUrl", { defaultValue: "Base URL" })}
        </FormLabel>
        <ImeSafeInput
          id="opencode-baseurl"
          value={baseUrl}
          onValueChange={onBaseUrlChange}
          placeholder="https://api.example.com/v1"
        />
        <p className="text-xs text-muted-foreground">
          {t("opencode.baseUrlHint", {
            defaultValue:
              "The base URL for the API endpoint. Leave empty to use the default endpoint for official SDKs.",
          })}
        </p>
      </div>

      <RequestHeadersEditor
        headers={headers}
        onHeadersChange={onHeadersChange}
      />

      {/* Extra Options Editor */}
      <div className="space-y-2 border-l border-border-default pl-3">
        <div className="flex items-start justify-between gap-3">
          <div className="max-w-3xl space-y-1">
            <FormLabel>
              {t("opencode.extraOptions", {
                defaultValue: "Extra SDK Options",
              })}
            </FormLabel>
            <p className="text-xs text-muted-foreground">
              {t("opencode.extraOptionsHint", {
                defaultValue:
                  "Advanced SDK options not exposed by the structured fields.",
              })}
            </p>
          </div>
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={handleAddExtraOption}
            className="h-7 gap-1"
          >
            <Plus className="h-3.5 w-3.5" />
            {t("opencode.addExtraOption", { defaultValue: "Add" })}
          </Button>
        </div>

        <div className="max-w-3xl">
          {Object.keys(extraOptions).length === 0 ? (
            <p className="text-sm text-muted-foreground py-1">
              {t("opencode.noExtraOptions", {
                defaultValue: "No extra SDK options configured",
              })}
            </p>
          ) : (
            <div className="space-y-2">
              <div className="flex items-center gap-2 text-xs text-muted-foreground px-1 mb-1">
                <span className="flex-1">
                  {t("opencode.extraOptionKey", { defaultValue: "Key" })}
                </span>
                <span className="flex-1">
                  {t("opencode.extraOptionValue", { defaultValue: "Value" })}
                </span>
                <span className="w-9" />
              </div>
              {Object.entries(extraOptions).map(([key, value]) => (
                <div key={key} className="flex items-center gap-2">
                  <ExtraOptionKeyInput
                    optionKey={key}
                    onChange={(newKey) =>
                      handleExtraOptionKeyChange(key, newKey)
                    }
                    placeholder={t("opencode.extraOptionKeyPlaceholder", {
                      defaultValue: "timeout",
                    })}
                  />
                  <ImeSafeInput
                    value={value}
                    onValueChange={(nextValue) =>
                      handleExtraOptionValueChange(key, nextValue)
                    }
                    placeholder={t("opencode.extraOptionValuePlaceholder", {
                      defaultValue: "600000",
                    })}
                    className="flex-1"
                  />
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon"
                    onClick={() => handleRemoveExtraOption(key)}
                    className="h-9 w-9 text-muted-foreground hover:text-destructive"
                  >
                    <Trash2 className="h-4 w-4" />
                  </Button>
                </div>
              ))}
            </div>
          )}
        </div>
      </div>

      {/* Models Editor */}
      <div className="space-y-3 border-l border-border-default pl-3">
        <div className="flex items-center justify-between">
          <FormLabel>
            {t("opencode.models", { defaultValue: "Models" })}
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
              {t("opencode.addModel", { defaultValue: "Add" })}
            </Button>
          </div>
        </div>

        {/* 思考档位名称偏好 + 自动填充模型能力：仅在已有模型时显示 */}
        {Object.keys(models).length > 0 && (
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div className="flex items-center gap-2">
              <FormLabel
                htmlFor="opencode-fill-style"
                className="text-xs text-muted-foreground"
              >
                {t("opencode.thinkingLevelNameStyleLabel", {
                  defaultValue: "Thinking level name style",
                })}
              </FormLabel>
              <Select
                value={variantsStyle}
                onValueChange={(value) =>
                  handleVariantsStyleChange(value as ModelsDevVariantsStyle)
                }
                disabled={!settingsData}
              >
                <SelectTrigger
                  id="opencode-fill-style"
                  className="h-7 w-44 text-xs"
                >
                  <SelectValue />
                </SelectTrigger>
                <SelectContent className="z-[200]">
                  <SelectItem value="plain">
                    {t("opencode.fillStylePlain", {
                      defaultValue: "Plain names",
                    })}
                  </SelectItem>
                  <SelectItem value="numbered">
                    {t("opencode.fillStyleNumbered", {
                      defaultValue: "Numbered prefixes",
                    })}
                  </SelectItem>
                </SelectContent>
              </Select>
            </div>
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={() => handleFillFromModelsDev()}
              disabled={isFetchingCapabilities}
              className="h-7 gap-1.5 whitespace-nowrap"
            >
              {isFetchingCapabilities ? (
                <Loader2 className="h-3.5 w-3.5 shrink-0 animate-spin" />
              ) : (
                <Sparkles className="h-3.5 w-3.5 shrink-0" />
              )}
              {t("opencode.autoFillCapabilities", {
                defaultValue: "Auto-fill capabilities",
              })}
            </Button>
          </div>
        )}

        {Object.keys(models).length === 0 ? (
          <p className="text-sm text-muted-foreground py-2">
            {t("opencode.noModels", {
              defaultValue: "No models configured. Click Add to add a model.",
            })}
          </p>
        ) : (
          <div className="space-y-2">
            <div className="flex items-center gap-2 text-xs text-muted-foreground px-1 mb-1">
              <span className="w-9" />
              <span className="flex-1">
                {t("opencode.modelId", { defaultValue: "模型 ID" })}
              </span>
              <span className="flex-1">
                {t("opencode.modelName", { defaultValue: "显示名称" })}
              </span>
              <span className="w-9" />
            </div>
            {Object.entries(models).map(([key, model]) => (
              <div key={key} className="space-y-2">
                {/* Model row */}
                <div className="flex items-center gap-2">
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon"
                    onClick={() => toggleModelExpand(key)}
                    aria-label={t("opencode.toggleModelDetails", {
                      defaultValue: "Toggle model details",
                    })}
                    className="h-9 w-9 shrink-0"
                  >
                    <ChevronRight
                      className={cn(
                        "h-4 w-4 transition-transform",
                        expandedModels.has(key) && "rotate-90",
                      )}
                    />
                  </Button>
                  <div className="flex gap-1 flex-1">
                    <ModelIdInput
                      modelId={key}
                      onChange={(newId) => handleModelIdChange(key, newId)}
                      placeholder={t("opencode.modelId", {
                        defaultValue: "Model ID",
                      })}
                    />
                    {fetchedModels.length > 0 && (
                      <ModelDropdown
                        models={fetchedModels}
                        onSelect={(id) => handleModelIdChange(key, id)}
                      />
                    )}
                  </div>
                  <ImeSafeInput
                    value={model.name}
                    onValueChange={(value) => handleModelNameChange(key, value)}
                    placeholder={t("opencode.modelName", {
                      defaultValue: "Display Name",
                    })}
                    className="flex-1"
                  />
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon"
                    onClick={() => handleRemoveModel(key)}
                    className="h-9 w-9 text-muted-foreground hover:text-destructive"
                  >
                    <Trash2 className="h-4 w-4" />
                  </Button>
                </div>

                {/* Expanded model details */}
                {expandedModels.has(key) && (
                  <div className="ml-9 pl-4 border-l-2 border-muted space-y-3">
                    {/* Capability switches (model.reasoning / model.modalities.input) */}
                    <div className="flex flex-wrap items-center gap-x-8 gap-y-2">
                      <div className="flex items-center gap-2.5">
                        <Switch
                          id={`opencode-model-reasoning-${key}`}
                          checked={model.reasoning === true}
                          onCheckedChange={(checked) =>
                            handleModelReasoningChange(key, checked)
                          }
                        />
                        <label
                          htmlFor={`opencode-model-reasoning-${key}`}
                          className="cursor-pointer text-sm"
                        >
                          {t("opencode.supportsReasoning", {
                            defaultValue: "Supports extended thinking",
                          })}
                        </label>
                      </div>
                      <div className="flex items-center gap-2.5">
                        <Switch
                          id={`opencode-model-image-${key}`}
                          checked={supportsImageInput(model)}
                          onCheckedChange={(checked) =>
                            handleModelImageInputChange(key, checked)
                          }
                        />
                        <label
                          htmlFor={`opencode-model-image-${key}`}
                          className="cursor-pointer text-sm"
                        >
                          {t("opencode.supportsImageInput", {
                            defaultValue: "Supports image input",
                          })}
                        </label>
                      </div>
                    </div>

                    {/* Token limits (model.limit) */}
                    <div className="space-y-2">
                      <span className="text-xs font-medium text-muted-foreground">
                        {t("opencode.modelLimits", {
                          defaultValue: "Token Limits",
                        })}
                      </span>
                      <div className="grid grid-cols-1 sm:grid-cols-2 gap-2">
                        <div className="space-y-1">
                          <FormLabel
                            htmlFor={`opencode-${key}-limit-context`}
                            className="text-xs text-muted-foreground"
                          >
                            {t("opencode.limitContext", {
                              defaultValue: "Context",
                            })}
                          </FormLabel>
                          <Input
                            id={`opencode-${key}-limit-context`}
                            type="number"
                            min={0}
                            step={1}
                            value={model.limit?.context ?? ""}
                            onChange={(e) =>
                              handleModelLimitChange(
                                key,
                                "context",
                                e.target.value,
                              )
                            }
                            placeholder="1048576"
                          />
                        </div>
                        <div className="space-y-1">
                          <FormLabel
                            htmlFor={`opencode-${key}-limit-output`}
                            className="text-xs text-muted-foreground"
                          >
                            {t("opencode.limitOutput", {
                              defaultValue: "Output",
                            })}
                          </FormLabel>
                          <Input
                            id={`opencode-${key}-limit-output`}
                            type="number"
                            min={0}
                            step={1}
                            value={model.limit?.output ?? ""}
                            onChange={(e) =>
                              handleModelLimitChange(
                                key,
                                "output",
                                e.target.value,
                              )
                            }
                            placeholder="131072"
                          />
                        </div>
                      </div>
                    </div>

                    {/* Thinking levels (model.variants), below Token Limits */}
                    {model.reasoning === true && (
                      <ThinkingLevelsEditor
                        modelId={key}
                        npm={npm}
                        outputLimit={
                          typeof model.limit?.output === "number"
                            ? model.limit.output
                            : undefined
                        }
                        variants={
                          (model.variants ?? {}) as Record<
                            string,
                            Record<string, unknown>
                          >
                        }
                        onChange={(nextVariants) =>
                          handleModelVariantsChange(key, nextVariants)
                        }
                        style={variantsStyle}
                      />
                    )}

                    {/* Model Properties (extra fields like cost), collapsed by default */}
                    <div className="space-y-2">
                      <div className="flex items-center justify-between">
                        <button
                          type="button"
                          onClick={() =>
                            setExtraFieldsExpanded((prev) => !prev)
                          }
                          className="inline-flex items-center gap-1 text-xs font-medium text-muted-foreground transition-colors hover:text-foreground"
                        >
                          <ChevronRight
                            className={cn(
                              "h-3.5 w-3.5 transition-transform",
                              extraFieldsExpanded && "rotate-90",
                            )}
                          />
                          {t("opencode.modelExtraFields", {
                            defaultValue: "模型属性",
                          })}
                        </button>
                        <Button
                          type="button"
                          variant="ghost"
                          size="sm"
                          onClick={() => {
                            setExtraFieldsExpanded(true);
                            handleAddModelExtraField(key);
                          }}
                          className="h-6 px-2 gap-1"
                        >
                          <Plus className="h-3 w-3" />
                        </Button>
                      </div>
                      {extraFieldsExpanded &&
                        (Object.keys(getModelExtraFields(model)).length ===
                        0 ? (
                          <p className="text-xs text-muted-foreground py-1">
                            {t("opencode.noModelExtraFields", {
                              defaultValue: "模型属性 (cost 等)，点击 + 添加",
                            })}
                          </p>
                        ) : (
                          Object.entries(getModelExtraFields(model)).map(
                            ([fKey, fValue]) => (
                              <div
                                key={fKey}
                                className="flex items-center gap-2"
                              >
                                <ModelOptionKeyInput
                                  optionKey={fKey}
                                  onChange={(newKey) =>
                                    handleModelExtraFieldKeyChange(
                                      key,
                                      fKey,
                                      newKey,
                                    )
                                  }
                                  placeholder={t(
                                    "opencode.modelExtraFieldKeyPlaceholder",
                                    {
                                      defaultValue: "cost",
                                    },
                                  )}
                                />
                                <ImeSafeInput
                                  value={fValue}
                                  onValueChange={(value) =>
                                    handleModelExtraFieldValueChange(
                                      key,
                                      fKey,
                                      value,
                                    )
                                  }
                                  placeholder={t(
                                    "opencode.modelOptionValuePlaceholder",
                                    {
                                      defaultValue: '{"order": ["baseten"]}',
                                    },
                                  )}
                                  className="flex-1"
                                />
                                <Button
                                  type="button"
                                  variant="ghost"
                                  size="icon"
                                  onClick={() =>
                                    handleRemoveModelExtraField(key, fKey)
                                  }
                                  className="h-9 w-9 text-muted-foreground hover:text-destructive"
                                >
                                  <Trash2 className="h-4 w-4" />
                                </Button>
                              </div>
                            ),
                          )
                        ))}
                    </div>

                    {/* SDK Options (model.options) */}
                    <div className="space-y-2">
                      <div className="flex items-center justify-between">
                        <span className="text-xs font-medium text-muted-foreground">
                          {t("opencode.sdkOptions", {
                            defaultValue: "SDK 选项",
                          })}
                        </span>
                        <Button
                          type="button"
                          variant="ghost"
                          size="sm"
                          onClick={() => handleAddModelOption(key)}
                          className="h-6 px-2 gap-1"
                        >
                          <Plus className="h-3 w-3" />
                        </Button>
                      </div>
                      {Object.keys(model.options || {}).length === 0 ? (
                        <p className="text-xs text-muted-foreground py-1">
                          {t("opencode.noModelOptions", {
                            defaultValue: "模型选项，点击 + 添加",
                          })}
                        </p>
                      ) : (
                        Object.entries(model.options || {}).map(
                          ([optKey, optValue]) => (
                            <div
                              key={optKey}
                              className="flex items-center gap-2"
                            >
                              <ModelOptionKeyInput
                                optionKey={optKey}
                                onChange={(newKey) =>
                                  handleModelOptionKeyChange(
                                    key,
                                    optKey,
                                    newKey,
                                  )
                                }
                                placeholder={t(
                                  "opencode.modelOptionKeyPlaceholder",
                                  {
                                    defaultValue: "provider",
                                  },
                                )}
                              />
                              <ImeSafeInput
                                value={
                                  typeof optValue === "string"
                                    ? optValue
                                    : JSON.stringify(optValue)
                                }
                                onValueChange={(value) =>
                                  handleModelOptionValueChange(
                                    key,
                                    optKey,
                                    value,
                                  )
                                }
                                placeholder={t(
                                  "opencode.modelOptionValuePlaceholder",
                                  {
                                    defaultValue: '{"order": ["baseten"]}',
                                  },
                                )}
                                className="flex-1"
                              />
                              <Button
                                type="button"
                                variant="ghost"
                                size="icon"
                                onClick={() =>
                                  handleRemoveModelOption(key, optKey)
                                }
                                className="h-9 w-9 text-muted-foreground hover:text-destructive"
                              >
                                <Trash2 className="h-4 w-4" />
                              </Button>
                            </div>
                          ),
                        )
                      )}
                    </div>
                  </div>
                )}
              </div>
            ))}
          </div>
        )}

        <p className="text-xs text-muted-foreground">
          {t("opencode.modelsHint", {
            defaultValue:
              "Configure available models. Model ID is the API identifier, Display Name is shown in the UI.",
          })}
        </p>
      </div>
    </>
  );
}
