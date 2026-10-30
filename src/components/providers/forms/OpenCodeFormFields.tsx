import { useState, useEffect, useCallback, useRef } from "react";
import { useTranslation } from "react-i18next";
import { FormLabel } from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { ImeSafeInput } from "@/components/ui/ime-safe-input";
import { Button } from "@/components/ui/button";
import { HoverTip } from "@/components/ui/hover-tip";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { toast } from "@/lib/toast";
import { Plus, Trash2, ChevronDown, ChevronRight } from "lucide-react";
import { ApiKeySection, GptContextWindowHint, ModelDropdown } from "./shared";
import {
  fetchModelsForConfig,
  showFetchModelsError,
  type FetchedModel,
} from "@/lib/api/model-fetch";
import { opencodeNpmPackages } from "@/config/opencodeProviderPresets";
import { cn } from "@/lib/utils";
import { isPlainObject } from "@/lib/requestOverrides";
import {
  getModelExtraFields,
  isKnownModelKey,
  isReasoningEffortMap,
  modelSupportsImageInput,
  modelSupportsReasoning,
  OPENCODE_EXTRA_OPTION_DRAFT_PREFIX,
  withModelImageInput,
  withModelReasoning,
  type OpenCodeModelDialect,
} from "./helpers/opencodeFormUtils";
import { StructuredOptionsEditor } from "./StructuredOptionsEditor";
import { PI_THINKING_LEVELS } from "@/config/piThinkingProfiles";
import { RequestHeadersEditor } from "./RequestHeadersEditor";
import {
  findRequestHeaderValue,
  normalizeRequestHeaders,
} from "./helpers/requestHeaders";
import {
  FetchedModelPicker,
  type FetchedModelSelection,
} from "./FetchedModelPicker";
import type { ProviderCategory, OpenCodeModel } from "@/types";
import { useCommittableRef } from "@/hooks/useLatestRef";
import { useModelMetadataFill } from "@/hooks/useModelMetadataFill";
import type { PresetModelSource } from "@/lib/modelMetadata";
import { opencodePresetModelSources } from "@/config/presetModelMetadata";
import { fillOpenCodeModel, metadataFilledAnything } from "./modelMetadataFill";

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

/**
 * DeepSeek Harness 的思考档位：列出的档位才提供，值是发给上游的档位名（null 表示不带）。
 * 新打开的档位按同名透传；不在 Pi 档位表里的键原样保留。
 */
function ReasoningEffortsField({
  modelKey,
  efforts,
  onChange,
}: {
  modelKey: string;
  efforts: Record<string, string | null>;
  onChange: (efforts: Record<string, string | null>) => void;
}) {
  const { t } = useTranslation();
  const [expanded, setExpanded] = useState(false);
  const offeredCount = Object.keys(efforts).length;

  const toggle = (level: string, offered: boolean) => {
    const next: Record<string, string | null> = {};
    for (const known of PI_THINKING_LEVELS) {
      if (known === level) {
        if (offered)
          next[known] =
            known in efforts ? efforts[known] : known === "off" ? null : known;
      } else if (known in efforts) {
        next[known] = efforts[known];
      }
    }
    for (const [other, value] of Object.entries(efforts)) {
      if (!(other in next) && other !== level) next[other] = value;
    }
    onChange(next);
  };

  return (
    <div className="w-full space-y-2 sm:col-span-2">
      <div className="flex min-h-9 items-center">
        <Button
          type="button"
          variant="ghost"
          size="sm"
          onClick={() => setExpanded((current) => !current)}
          aria-expanded={expanded}
          className="-ml-2 h-8 gap-1.5 px-2 text-fg-1"
        >
          <span>{t("pi.form.thinkingLevelsLabel")}</span>
          <ChevronDown
            className={`h-4 w-4 transition-transform motion-reduce:transition-none ${
              expanded ? "rotate-180" : ""
            }`}
          />
        </Button>
      </div>
      {expanded && (
        <div className="overflow-hidden rounded-lg border border-border/70 bg-surface">
          {PI_THINKING_LEVELS.map((level) => {
            const offered = level in efforts;
            const value = efforts[level];
            const id = `opencode-${modelKey}-effort-${level}`;
            return (
              <div
                key={level}
                className="flex h-[42px] items-center gap-3 border-b border-border/40 px-4 text-sm last:border-b-0"
              >
                <Label htmlFor={id} className="flex-1 cursor-pointer">
                  {t(`pi.form.thinkingLevels.${level}`)}
                </Label>
                <span
                  className={
                    offered
                      ? "max-w-[18rem] truncate font-mono text-xs text-fg-1"
                      : "text-xs text-fg-2"
                  }
                >
                  {offered
                    ? (value ?? "")
                    : t("pi.form.thinkingLevelUnsupported")}
                </span>
                <Switch
                  id={id}
                  checked={offered}
                  // 至少留一个档位，全关请用上面的「支持扩展思考」
                  disabled={offered && offeredCount === 1}
                  onCheckedChange={(checked) => toggle(level, checked)}
                />
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}

interface OpenCodeFormFieldsProps {
  allowBuiltinDefaults?: boolean;
  /** 模型字段方言：DeepSeek Harness 多出思考档位和接口兼容性。 */
  modelDialect?: OpenCodeModelDialect;
  /** 选中拉取到的模型时补参数所查的预设（MiniMax Code 传自己的）。 */
  presetModelSources?: () => readonly PresetModelSource[];
  apiFormats?: ReadonlyArray<{ value: string; label: string }>;
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
  allowBuiltinDefaults = false,
  modelDialect = "opencode",
  presetModelSources = opencodePresetModelSources,
  apiFormats = opencodeNpmPackages,
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
  const modelFetchGeneration = useRef(0);

  useEffect(() => {
    setFetchedModels((prev) => (prev.length === 0 ? prev : []));
    setIsFetchingModels(false);
    return () => {
      // Ignore responses for a previous endpoint/key or an unmounted form.
      modelFetchGeneration.current += 1;
    };
  }, [baseUrl, apiKey, headers]);

  const handleFetchModels = useCallback(() => {
    const requestHeaders = normalizeRequestHeaders(headers);
    const hasCredentials =
      Boolean(apiKey) || Object.keys(requestHeaders).length > 0;
    if (!baseUrl || !hasCredentials) {
      showFetchModelsError(null, t, {
        hasApiKey: hasCredentials,
        hasBaseUrl: Boolean(baseUrl),
      });
      return;
    }

    const customUserAgent = findRequestHeaderValue(
      requestHeaders,
      "user-agent",
    );

    const generation = ++modelFetchGeneration.current;
    setFetchedModels([]);
    setIsFetchingModels(true);
    fetchModelsForConfig(
      baseUrl,
      apiKey,
      undefined,
      undefined,
      customUserAgent,
      { requestHeaders },
    )
      .then((result) => {
        if (generation !== modelFetchGeneration.current) return;
        const models = [
          ...new Map(result.map((model) => [model.id, model])).values(),
        ];
        setFetchedModels(models);
        // 拉到模型时直接打开选择弹窗，不再提示成功。
        if (models.length === 0) {
          toast.info(t("providerForm.fetchModelsEmpty"));
        }
      })
      .catch((err) => {
        if (generation !== modelFetchGeneration.current) return;
        console.warn("[ModelFetch] Failed:", err);
        showFetchModelsError(err, t);
      })
      .finally(() => {
        if (generation === modelFetchGeneration.current) {
          setIsFetchingModels(false);
        }
      });
  }, [baseUrl, apiKey, headers, t]);

  // Track which models have expanded options panel
  const [expandedModels, setExpandedModels] = useState<Set<string>>(new Set());

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

  // 选中拉取到的模型时补上它已知的 limit 和模态（只补空着的）。
  // 补全可能晚到，要用最新的列表和回调提交。
  const [modelsRef, commitModels] = useCommittableRef(models, onModelsChange);
  const fillModelMetadata = useModelMetadataFill({
    baseUrl,
    presets: presetModelSources,
    prefetch: fetchedModels.length > 0,
  });
  const fillModelMetadataFor = (id: string, silent?: boolean) =>
    fillModelMetadata(
      id,
      (metadata) => {
        const current = modelsRef.current;
        if (!Object.prototype.hasOwnProperty.call(current, id)) return false;
        const filled = fillOpenCodeModel(current[id], metadata);
        if (!metadataFilledAnything(current[id], filled)) return false;
        commitModels({ ...current, [id]: filled });
        return true;
      },
      { silent },
    );

  const handleApplyFetchedModels = ({ add, remove }: FetchedModelSelection) => {
    const next = { ...models };
    for (const id of remove) delete next[id];
    const additions = Object.fromEntries(
      add
        .filter((id) => !Object.prototype.hasOwnProperty.call(next, id))
        .map((id) => [id, { name: id }]),
    );
    commitModels({ ...next, ...additions });
    if (remove.length > 0) {
      setExpandedModels((prev) => {
        const kept = new Set(prev);
        for (const id of remove) kept.delete(id);
        return kept;
      });
    }
    // 弹窗里已经显示过这些参数，补完不再提示。
    for (const id of Object.keys(additions)) fillModelMetadataFor(id, true);
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
    commitModels(newModels);
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

  const updateModel = (modelKey: string, model: OpenCodeModel) =>
    onModelsChange({ ...models, [modelKey]: model });

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
    const trimmed = newKey.trim();
    // Renaming onto an existing key collapses two rows into one string map,
    // which can no longer tell "the target row was replaced" from "it was
    // untouched", so the merge would silently keep the target's stale value
    // type. Reject like the model extra-field editor does.
    if (
      trimmed &&
      trimmed !== oldKey &&
      Object.prototype.hasOwnProperty.call(extraOptions, trimmed)
    ) {
      return false;
    }
    const newOptions: Record<string, string> = {};
    for (const [k, v] of Object.entries(extraOptions)) {
      if (k === oldKey) {
        newOptions[trimmed || oldKey] = v;
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
              placeholder={
                allowBuiltinDefaults
                  ? t("opencode.builtinDefaults")
                  : t("opencode.selectPackage", {
                      defaultValue: "Select a package",
                    })
              }
            />
          </SelectTrigger>
          <SelectContent>
            {apiFormats.map((pkg) => (
              <SelectItem key={pkg.value} value={pkg.value}>
                {pkg.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
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
      </div>

      <RequestHeadersEditor
        headers={headers}
        onHeadersChange={onHeadersChange}
      />

      {/* Extra Options Editor */}
      <div className="space-y-2 border-l border-border pl-3">
        <div className="flex items-start justify-between gap-3">
          <div className="space-y-1">
            <FormLabel>
              {t("opencode.extraOptions", {
                defaultValue: "Extra SDK Options",
              })}
            </FormLabel>
            <p className="text-xs text-fg-2">
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

        <div>
          {Object.keys(extraOptions).length === 0 ? (
            <p className="text-sm text-fg-2 py-1">
              {t("opencode.noExtraOptions", {
                defaultValue: "No extra SDK options configured",
              })}
            </p>
          ) : (
            <div className="space-y-2">
              <div className="flex items-center gap-2 text-xs text-fg-2 px-1 mb-1">
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
                  <HoverTip content={t("common.delete")}>
                    <Button
                      aria-label={t("common.delete")}
                      type="button"
                      variant="ghost"
                      size="icon"
                      onClick={() => handleRemoveExtraOption(key)}
                      className="h-9 w-9 text-fg-2 hover:text-destructive"
                    >
                      <Trash2 className="h-4 w-4" />
                    </Button>
                  </HoverTip>
                </div>
              ))}
            </div>
          )}
        </div>
      </div>

      {/* Models Editor */}
      <div className="space-y-3 border-l border-border pl-3">
        <div className="flex items-center justify-between">
          <FormLabel>
            {t("opencode.models", { defaultValue: "Models" })}
          </FormLabel>
          <div className="flex gap-1">
            <FetchedModelPicker
              models={fetchedModels}
              configuredModelIds={Object.keys(models)}
              onApply={handleApplyFetchedModels}
              onFetch={handleFetchModels}
              isFetching={isFetchingModels}
              metadataSource={{ baseUrl, presets: presetModelSources }}
            />
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={handleAddModel}
              className="h-7 gap-1"
            >
              <Plus className="h-3.5 w-3.5" />
              {t("opencode.addModel", { defaultValue: "Add manually" })}
            </Button>
          </div>
        </div>

        {Object.keys(models).length === 0 ? (
          <p className="text-sm text-fg-2 py-2">
            {t("opencode.noModels", {
              defaultValue: "No models configured. Click Add to add a model.",
            })}
          </p>
        ) : (
          <div className="space-y-2">
            <div className="flex items-center gap-2 text-xs text-fg-2 px-1 mb-1">
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
                        onSelect={(id) => {
                          handleModelIdChange(key, id);
                          fillModelMetadataFor(id);
                        }}
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
                  <HoverTip content={t("common.delete")}>
                    <Button
                      aria-label={t("common.delete")}
                      type="button"
                      variant="ghost"
                      size="icon"
                      onClick={() => handleRemoveModel(key)}
                      className="h-9 w-9 text-fg-2 hover:text-destructive"
                    >
                      <Trash2 className="h-4 w-4" />
                    </Button>
                  </HoverTip>
                </div>

                {/* Expanded model details */}
                {expandedModels.has(key) && (
                  <div className="ml-9 grid gap-3 border-l-2 border-muted pl-4 sm:grid-cols-2">
                    <div className="flex min-h-9 flex-wrap items-center gap-x-8 gap-y-2 sm:col-span-2">
                      <div className="flex items-center gap-2.5">
                        <Label
                          htmlFor={`opencode-${key}-reasoning`}
                          className="cursor-pointer"
                        >
                          {t("pi.form.reasoning")}
                        </Label>
                        <Switch
                          id={`opencode-${key}-reasoning`}
                          checked={modelSupportsReasoning(model, modelDialect)}
                          onCheckedChange={(checked) =>
                            updateModel(
                              key,
                              withModelReasoning(model, checked, modelDialect),
                            )
                          }
                        />
                      </div>
                      <div className="flex items-center gap-2.5">
                        <Label
                          htmlFor={`opencode-${key}-image-input`}
                          className="cursor-pointer"
                        >
                          {t("pi.form.imageInput")}
                        </Label>
                        <Switch
                          id={`opencode-${key}-image-input`}
                          checked={modelSupportsImageInput(model)}
                          onCheckedChange={(checked) =>
                            updateModel(
                              key,
                              withModelImageInput(model, checked),
                            )
                          }
                        />
                      </div>
                    </div>
                    <div className="space-y-2">
                      <Label htmlFor={`opencode-${key}-limit-context`}>
                        {t("pi.form.contextWindow")}
                      </Label>
                      <Input
                        id={`opencode-${key}-limit-context`}
                        type="number"
                        min={0}
                        step={1}
                        value={model.limit?.context ?? ""}
                        onChange={(e) =>
                          handleModelLimitChange(key, "context", e.target.value)
                        }
                        placeholder="1048576"
                      />
                      <GptContextWindowHint
                        modelId={key}
                        contextWindow={model.limit?.context}
                      />
                    </div>
                    <div className="space-y-2">
                      <Label htmlFor={`opencode-${key}-limit-output`}>
                        {t("pi.form.maxTokens")}
                      </Label>
                      <Input
                        id={`opencode-${key}-limit-output`}
                        type="number"
                        min={0}
                        step={1}
                        value={model.limit?.output ?? ""}
                        onChange={(e) =>
                          handleModelLimitChange(key, "output", e.target.value)
                        }
                        placeholder="131072"
                      />
                    </div>

                    {modelDialect === "dsh" &&
                      isReasoningEffortMap(model.reasoningEfforts) && (
                        <ReasoningEffortsField
                          modelKey={key}
                          efforts={model.reasoningEfforts}
                          onChange={(reasoningEfforts) =>
                            updateModel(key, { ...model, reasoningEfforts })
                          }
                        />
                      )}

                    {modelDialect === "dsh" && isPlainObject(model.compat) && (
                      <StructuredOptionsEditor
                        id={`opencode-${key}-compat`}
                        title={t("pi.form.compatibility")}
                        hint={t("pi.form.compatibilityHint")}
                        addLabel={t("pi.form.addCompatibilityOption")}
                        emptyLabel={t("pi.form.noCompatibilityOptions")}
                        keyLabel={t("pi.form.optionKey")}
                        valueLabel={t("pi.form.optionValue")}
                        keyPlaceholder="supportsDeveloperRole"
                        valuePlaceholder="false"
                        removeLabel={t("pi.form.removeCompatibilityOption")}
                        options={model.compat}
                        onOptionsChange={(compat) => {
                          const { compat: _previous, ...rest } = model;
                          updateModel(
                            key,
                            Object.keys(compat).length > 0
                              ? { ...rest, compat }
                              : (rest as OpenCodeModel),
                          );
                        }}
                        className="sm:col-span-2"
                      />
                    )}

                    {/* Model Properties (extra fields like variants, cost) */}
                    <div className="space-y-2 sm:col-span-2">
                      <div className="flex items-center justify-between">
                        <span className="text-xs font-medium text-fg-2">
                          {t("opencode.modelExtraFields", {
                            defaultValue: "模型属性",
                          })}
                        </span>
                        <Button
                          type="button"
                          variant="ghost"
                          size="sm"
                          onClick={() => handleAddModelExtraField(key)}
                          className="h-6 px-2 gap-1"
                        >
                          <Plus className="h-3 w-3" />
                        </Button>
                      </div>
                      {Object.keys(getModelExtraFields(model, modelDialect))
                        .length === 0 ? (
                        <p className="text-xs text-fg-2 py-1">
                          {t("opencode.noModelExtraFields", {
                            defaultValue:
                              "模型属性 (variants, cost 等)，点击 + 添加",
                          })}
                        </p>
                      ) : (
                        Object.entries(
                          getModelExtraFields(model, modelDialect),
                        ).map(([fKey, fValue]) => (
                          <div key={fKey} className="flex items-center gap-2">
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
                                  defaultValue: "variants",
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
                            <HoverTip content={t("common.delete")}>
                              <Button
                                aria-label={t("common.delete")}
                                type="button"
                                variant="ghost"
                                size="icon"
                                onClick={() =>
                                  handleRemoveModelExtraField(key, fKey)
                                }
                                className="h-9 w-9 text-fg-2 hover:text-destructive"
                              >
                                <Trash2 className="h-4 w-4" />
                              </Button>
                            </HoverTip>
                          </div>
                        ))
                      )}
                    </div>

                    {/* SDK Options (model.options) */}
                    <div className="space-y-2 sm:col-span-2">
                      <div className="flex items-center justify-between">
                        <span className="text-xs font-medium text-fg-2">
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
                        <p className="text-xs text-fg-2 py-1">
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
                              <HoverTip content={t("common.delete")}>
                                <Button
                                  aria-label={t("common.delete")}
                                  type="button"
                                  variant="ghost"
                                  size="icon"
                                  onClick={() =>
                                    handleRemoveModelOption(key, optKey)
                                  }
                                  className="h-9 w-9 text-fg-2 hover:text-destructive"
                                >
                                  <Trash2 className="h-4 w-4" />
                                </Button>
                              </HoverTip>
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
      </div>
    </>
  );
}
