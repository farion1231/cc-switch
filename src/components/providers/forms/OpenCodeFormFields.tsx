import { useState, useEffect, useCallback } from "react";
import { useTranslation } from "react-i18next";
import { FormLabel } from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { toast } from "sonner";
import { Download, Plus, Trash2, ChevronRight, Loader2 } from "lucide-react";
import { ApiKeySection, ModelDropdown } from "./shared";
import {
  fetchModelsForConfig,
  showFetchModelsError,
  type FetchedModel,
} from "@/lib/api/model-fetch";
import { opencodeNpmPackages } from "@/config/opencodeProviderPresets";
import { cn } from "@/lib/utils";
import {
  getModelExtraFields,
  isKnownModelKey,
  OPENCODE_EXTRA_OPTION_DRAFT_PREFIX,
  OPENCODE_HEADER_DRAFT_PREFIX,
} from "./helpers/opencodeFormUtils";
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
    <Input
      value={localValue}
      onChange={(e) => setLocalValue(e.target.value)}
      onBlur={() => {
        if (localValue !== modelId && localValue.trim()) {
          onChange(localValue);
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
    <Input
      value={localValue}
      onChange={(e) => setLocalValue(e.target.value)}
      onBlur={() => {
        const trimmed = localValue.trim();
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
    <Input
      value={localValue}
      onChange={(e) => setLocalValue(e.target.value)}
      onBlur={() => {
        const trimmed = localValue.trim();
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
  const [extraOptionsOpen, setExtraOptionsOpen] = useState(
    () => Object.keys(extraOptions).length > 0,
  );

  useEffect(() => {
    if (Object.keys(extraOptions).length > 0) {
      setExtraOptionsOpen(true);
    }
  }, [extraOptions]);

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

  // Header handlers
  const handleAddHeader = () => {
    const newKey = `${OPENCODE_HEADER_DRAFT_PREFIX}${Date.now()}`;
    onHeadersChange({
      ...headers,
      [newKey]: "",
    });
  };

  const handleRemoveHeader = (key: string) => {
    const newHeaders = { ...headers };
    delete newHeaders[key];
    onHeadersChange(newHeaders);
  };

  const handleHeaderKeyChange = (oldKey: string, newKey: string): boolean => {
    const trimmedKey = newKey.trim();
    if (!trimmedKey || oldKey === trimmedKey) return false;

    const normalizedKey = trimmedKey.toLowerCase();
    const hasDuplicate = Object.keys(headers).some(
      (key) => key !== oldKey && key.toLowerCase() === normalizedKey,
    );
    if (hasDuplicate) return false;

    const newHeaders: Record<string, string> = {};
    for (const [key, value] of Object.entries(headers)) {
      if (key === oldKey) newHeaders[trimmedKey] = value;
      else newHeaders[key] = value;
    }
    onHeadersChange(newHeaders);
    return true;
  };

  const handleHeaderValueChange = (key: string, value: string) => {
    onHeadersChange({
      ...headers,
      [key]: value,
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
        <Input
          id="opencode-baseurl"
          value={baseUrl}
          onChange={(e) => onBaseUrlChange(e.target.value)}
          placeholder="https://api.example.com/v1"
        />
        <p className="text-xs text-muted-foreground">
          {t("opencode.baseUrlHint", {
            defaultValue:
              "The base URL for the API endpoint. Leave empty to use the default endpoint for official SDKs.",
          })}
        </p>
      </div>

      {/* Headers Editor */}
      <div className="space-y-2 border-l border-border-default pl-3">
        <div className="flex items-start justify-between gap-3">
          <div className="max-w-3xl space-y-1">
            <FormLabel>
              {t("opencode.headers", { defaultValue: "Headers" })}
            </FormLabel>
            <p className="text-xs text-muted-foreground">
              {t("opencode.headersHint", {
                defaultValue:
                  "Optional HTTP headers sent with provider requests, such as HTTP-Referer or X-Title.",
              })}
            </p>
          </div>
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={handleAddHeader}
            aria-label={t("opencode.addHeader", { defaultValue: "Add header" })}
            className="h-7 gap-1"
          >
            <Plus className="h-3.5 w-3.5" />
            {t("opencode.addHeader", { defaultValue: "Add" })}
          </Button>
        </div>

        <div className="max-w-3xl">
          {Object.keys(headers).length === 0 ? (
            <p className="text-sm text-muted-foreground py-1">
              {t("opencode.noHeaders", {
                defaultValue: "No custom headers configured",
              })}
            </p>
          ) : (
            <div className="space-y-2">
              <div className="flex items-center gap-2 text-xs text-muted-foreground px-1 mb-1">
                <span className="flex-1">
                  {t("opencode.headerName", { defaultValue: "Header" })}
                </span>
                <span className="flex-1">
                  {t("opencode.headerValue", { defaultValue: "Value" })}
                </span>
                <span className="w-9" />
              </div>
              {Object.entries(headers).map(([key, value]) => (
                <div key={key} className="flex items-center gap-2">
                  <ExtraOptionKeyInput
                    optionKey={key}
                    onChange={(newKey) => handleHeaderKeyChange(key, newKey)}
                    placeholder={t("opencode.headerNamePlaceholder", {
                      defaultValue: "X-Title",
                    })}
                    placeholderPrefixes={[OPENCODE_HEADER_DRAFT_PREFIX]}
                  />
                  <Input
                    value={value}
                    onChange={(e) =>
                      handleHeaderValueChange(key, e.target.value)
                    }
                    placeholder={t("opencode.headerValuePlaceholder", {
                      defaultValue: "CC Switch",
                    })}
                    className="flex-1"
                  />
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon"
                    onClick={() => handleRemoveHeader(key)}
                    aria-label={t("opencode.removeHeader", {
                      defaultValue: "Remove header",
                    })}
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

      {/* Extra Options Editor */}
      <Collapsible
        open={extraOptionsOpen}
        onOpenChange={setExtraOptionsOpen}
        className="space-y-2 border-l border-border-default pl-3"
      >
        <div className="flex items-start justify-between gap-3">
          <CollapsibleTrigger asChild>
            <button
              type="button"
              className="flex min-w-0 max-w-3xl flex-1 items-start gap-2 text-left"
            >
              <ChevronRight
                className={cn(
                  "mt-0.5 h-4 w-4 shrink-0 text-muted-foreground transition-transform",
                  extraOptionsOpen && "rotate-90",
                )}
              />
              <span className="space-y-1">
                <span className="block text-sm font-medium text-foreground">
                  {t("opencode.extraOptions", {
                    defaultValue: "Extra SDK Options",
                  })}
                </span>
                <span className="block text-xs text-muted-foreground">
                  {t("opencode.extraOptionsHint", {
                    defaultValue:
                      "Advanced SDK options not exposed by the structured fields.",
                  })}
                </span>
              </span>
            </button>
          </CollapsibleTrigger>
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={() => {
              setExtraOptionsOpen(true);
              handleAddExtraOption();
            }}
            className="h-7 gap-1"
          >
            <Plus className="h-3.5 w-3.5" />
            {t("opencode.addExtraOption", { defaultValue: "Add" })}
          </Button>
        </div>

        <CollapsibleContent className="max-w-3xl space-y-2">
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
                  <Input
                    value={value}
                    onChange={(e) =>
                      handleExtraOptionValueChange(key, e.target.value)
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
        </CollapsibleContent>
      </Collapsible>

      {/* Models Editor */}
      <div className="space-y-3">
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
                  <Input
                    value={model.name}
                    onChange={(e) => handleModelNameChange(key, e.target.value)}
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

                    {/* Model Properties (extra fields like variants, cost) */}
                    <div className="space-y-2">
                      <div className="flex items-center justify-between">
                        <span className="text-xs font-medium text-muted-foreground">
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
                      {Object.keys(getModelExtraFields(model)).length === 0 ? (
                        <p className="text-xs text-muted-foreground py-1">
                          {t("opencode.noModelExtraFields", {
                            defaultValue:
                              "模型属性 (variants, cost 等)，点击 + 添加",
                          })}
                        </p>
                      ) : (
                        Object.entries(getModelExtraFields(model)).map(
                          ([fKey, fValue]) => (
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
                              <Input
                                value={fValue}
                                onChange={(e) =>
                                  handleModelExtraFieldValueChange(
                                    key,
                                    fKey,
                                    e.target.value,
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
                      )}
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
                              <Input
                                value={
                                  typeof optValue === "string"
                                    ? optValue
                                    : JSON.stringify(optValue)
                                }
                                onChange={(e) =>
                                  handleModelOptionValueChange(
                                    key,
                                    optKey,
                                    e.target.value,
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
