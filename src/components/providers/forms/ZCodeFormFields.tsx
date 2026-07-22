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
import { Plus, Trash2, ChevronRight } from "lucide-react";
import { ApiKeySection } from "./shared";
import { zcodeProviderKinds } from "@/config/zcodeProviderPresets";
import { cn } from "@/lib/utils";
import { ZCODE_EXTRA_OPTION_DRAFT_PREFIX } from "./helpers/zcodeFormUtils";
import type { ProviderCategory, ZCodeModel } from "@/types";

/**
 * Model ID input with local state to prevent focus loss on key change.
 * Mirrors the OpenCode ModelIdInput pattern.
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
 */
function ExtraOptionKeyInput({
  optionKey,
  onChange,
  placeholder,
}: {
  optionKey: string;
  onChange: (newKey: string) => void;
  placeholder?: string;
}) {
  const isPlaceholderKey = optionKey.startsWith(
    ZCODE_EXTRA_OPTION_DRAFT_PREFIX,
  );
  const displayValue = isPlaceholderKey ? "" : optionKey;
  const [localValue, setLocalValue] = useState(displayValue);

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
          onChange(trimmed);
        }
      }}
      placeholder={placeholder}
      className="flex-1"
    />
  );
}

interface ZCodeFormFieldsProps {
  // Provider Kind
  kind: string;
  onKindChange: (value: string) => void;

  // Provider display name (config.json provider.<id>.name)
  name: string;
  onNameChange: (value: string) => void;

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

  // Models
  models: Record<string, ZCodeModel>;
  onModelsChange: (models: Record<string, ZCodeModel>) => void;

  // Extra Options
  extraOptions: Record<string, string>;
  onExtraOptionsChange: (options: Record<string, string>) => void;
}

export function ZCodeFormFields({
  kind,
  onKindChange,
  name,
  onNameChange,
  apiKey,
  onApiKeyChange,
  category,
  shouldShowApiKeyLink,
  websiteUrl,
  isPartner,
  partnerPromotionKey,
  baseUrl,
  onBaseUrlChange,
  models,
  onModelsChange,
  extraOptions,
  onExtraOptionsChange,
}: ZCodeFormFieldsProps) {
  const { t } = useTranslation();

  const [extraOptionsOpen, setExtraOptionsOpen] = useState(
    () => Object.keys(extraOptions).length > 0,
  );

  useEffect(() => {
    if (Object.keys(extraOptions).length > 0) {
      setExtraOptionsOpen(true);
    }
  }, [extraOptions]);

  // Track which models have expanded details panel
  const [expandedModels, setExpandedModels] = useState<Set<string>>(new Set());

  const toggleModelExpand = (key: string) => {
    setExpandedModels((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  };

  const handleAddModel = () => {
    const newKey = `model-${Date.now()}`;
    onModelsChange({
      ...models,
      [newKey]: { name: "" },
    });
  };

  const handleRemoveModel = (key: string) => {
    const newModels = { ...models };
    delete newModels[key];
    onModelsChange(newModels);
    setExpandedModels((prev) => {
      const next = new Set(prev);
      next.delete(key);
      return next;
    });
  };

  const handleModelIdChange = (oldKey: string, newKey: string) => {
    if (oldKey === newKey || !newKey.trim()) return;
    const newModels: Record<string, ZCodeModel> = {};
    for (const [k, v] of Object.entries(models)) {
      if (k === oldKey) {
        newModels[newKey] = v;
      } else {
        newModels[k] = v;
      }
    }
    onModelsChange(newModels);
    if (expandedModels.has(oldKey)) {
      setExpandedModels((prev) => {
        const next = new Set(prev);
        next.delete(oldKey);
        next.add(newKey);
        return next;
      });
    }
  };

  const handleModelNameChange = (key: string, modelName: string) => {
    onModelsChange({
      ...models,
      [key]: { ...models[key], name: modelName },
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

  // Extra Options handlers
  const handleAddExtraOption = () => {
    const newKey = `${ZCODE_EXTRA_OPTION_DRAFT_PREFIX}${Date.now()}`;
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

  const handleExtraOptionKeyChange = useCallback(
    (oldKey: string, newKey: string) => {
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
    },
    [extraOptions, onExtraOptionsChange],
  );

  const handleExtraOptionValueChange = (key: string, value: string) => {
    onExtraOptionsChange({
      ...extraOptions,
      [key]: value,
    });
  };

  return (
    <>
      {/* Provider Kind Selector */}
      <div className="space-y-2">
        <FormLabel htmlFor="zcode-kind">
          {t("zcode.kind", { defaultValue: "Provider Kind" })}
        </FormLabel>
        <Select value={kind} onValueChange={onKindChange}>
          <SelectTrigger id="zcode-kind">
            <SelectValue
              placeholder={t("zcode.kindPlaceholder", {
                defaultValue: "Select a provider kind",
              })}
            />
          </SelectTrigger>
          <SelectContent>
            {zcodeProviderKinds.map((k) => (
              <SelectItem key={k.value} value={k.value}>
                {k.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <p className="text-xs text-muted-foreground">
          {t("zcode.kindHint", {
            defaultValue:
              "Select the API protocol this provider speaks (Anthropic, OpenAI, or OpenAI-Compatible).",
          })}
        </p>
      </div>

      {/* Provider display name (optional) */}
      <div className="space-y-2">
        <FormLabel htmlFor="zcode-name">
          {t("zcode.providerName", { defaultValue: "Provider Name" })}
        </FormLabel>
        <Input
          id="zcode-name"
          value={name}
          onChange={(e) => onNameChange(e.target.value)}
          placeholder={t("zcode.providerNamePlaceholder", {
            defaultValue: "My Provider",
          })}
        />
        <p className="text-xs text-muted-foreground">
          {t("zcode.providerNameHint", {
            defaultValue:
              "Optional display name stored in config.json. Defaults to the provider key if left empty.",
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
        <FormLabel htmlFor="zcode-baseurl">
          {t("zcode.baseUrl", { defaultValue: "Base URL" })}
        </FormLabel>
        <Input
          id="zcode-baseurl"
          value={baseUrl}
          onChange={(e) => onBaseUrlChange(e.target.value)}
          placeholder="https://api.example.com/v1"
        />
        <p className="text-xs text-muted-foreground">
          {t("zcode.baseUrlHint", {
            defaultValue: "The base URL for the API endpoint.",
          })}
        </p>
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
                  {t("zcode.extraOptions", {
                    defaultValue: "Extra Options",
                  })}
                </span>
                <span className="block text-xs text-muted-foreground">
                  {t("zcode.extraOptionsHint", {
                    defaultValue:
                      "Advanced options not exposed by the structured fields.",
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
            {t("zcode.addExtraOption", { defaultValue: "Add" })}
          </Button>
        </div>

        <CollapsibleContent className="max-w-3xl space-y-2">
          {Object.keys(extraOptions).length === 0 ? (
            <p className="text-sm text-muted-foreground py-1">
              {t("zcode.noExtraOptions", {
                defaultValue: "No extra options configured",
              })}
            </p>
          ) : (
            <div className="space-y-2">
              <div className="flex items-center gap-2 text-xs text-muted-foreground px-1 mb-1">
                <span className="flex-1">
                  {t("zcode.extraOptionKey", { defaultValue: "Key" })}
                </span>
                <span className="flex-1">
                  {t("zcode.extraOptionValue", { defaultValue: "Value" })}
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
                    placeholder={t("zcode.extraOptionKeyPlaceholder", {
                      defaultValue: "timeout",
                    })}
                  />
                  <Input
                    value={value}
                    onChange={(e) =>
                      handleExtraOptionValueChange(key, e.target.value)
                    }
                    placeholder={t("zcode.extraOptionValuePlaceholder", {
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
          <FormLabel>{t("zcode.models", { defaultValue: "Models" })}</FormLabel>
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={handleAddModel}
            className="h-7 gap-1"
          >
            <Plus className="h-3.5 w-3.5" />
            {t("zcode.addModel", { defaultValue: "Add Model" })}
          </Button>
        </div>

        {Object.keys(models).length === 0 ? (
          <p className="text-sm text-muted-foreground py-2">
            {t("zcode.noModels", { defaultValue: "No models configured" })}
          </p>
        ) : (
          <div className="space-y-2">
            <div className="flex items-center gap-2 text-xs text-muted-foreground px-1 mb-1">
              <span className="w-9" />
              <span className="flex-1">
                {t("zcode.modelId", { defaultValue: "Model ID" })}
              </span>
              <span className="flex-1">
                {t("zcode.modelName", { defaultValue: "Display Name" })}
              </span>
              <span className="w-9" />
            </div>
            {Object.entries(models).map(([key, model]) => (
              <div key={key} className="space-y-2">
                <div className="flex items-center gap-2">
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon"
                    onClick={() => toggleModelExpand(key)}
                    aria-label={t("zcode.toggleModelDetails", {
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
                  <div className="flex-1">
                    <ModelIdInput
                      modelId={key}
                      onChange={(newId) => handleModelIdChange(key, newId)}
                      placeholder={t("zcode.modelId", {
                        defaultValue: "Model ID",
                      })}
                    />
                  </div>
                  <Input
                    value={model.name || ""}
                    onChange={(e) => handleModelNameChange(key, e.target.value)}
                    placeholder={t("zcode.modelName", {
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

                {expandedModels.has(key) && (
                  <div className="ml-9 pl-4 border-l-2 border-muted space-y-3">
                    <div className="space-y-2">
                      <span className="text-xs font-medium text-muted-foreground">
                        {t("zcode.modelLimits", {
                          defaultValue: "Token Limits",
                        })}
                      </span>
                      <div className="grid grid-cols-1 sm:grid-cols-2 gap-2">
                        <div className="space-y-1">
                          <FormLabel
                            htmlFor={`zcode-${key}-limit-context`}
                            className="text-xs text-muted-foreground"
                          >
                            {t("zcode.limitContext", {
                              defaultValue: "Context",
                            })}
                          </FormLabel>
                          <Input
                            id={`zcode-${key}-limit-context`}
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
                            htmlFor={`zcode-${key}-limit-output`}
                            className="text-xs text-muted-foreground"
                          >
                            {t("zcode.limitOutput", {
                              defaultValue: "Output",
                            })}
                          </FormLabel>
                          <Input
                            id={`zcode-${key}-limit-output`}
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
                  </div>
                )}
              </div>
            ))}
          </div>
        )}

        <p className="text-xs text-muted-foreground">
          {t("zcode.modelsHint", {
            defaultValue:
              "Configure available models. Model ID is the API identifier sent in requests.",
          })}
        </p>
      </div>
    </>
  );
}
