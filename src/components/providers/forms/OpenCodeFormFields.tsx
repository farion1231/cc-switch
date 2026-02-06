import { useState, useEffect } from "react";
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
import { Plus, Trash2, ChevronRight } from "lucide-react";
import { ApiKeySection } from "./shared";
import { opencodeNpmPackages } from "@/config/opencodeProviderPresets";
import { cn } from "@/lib/utils";
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
}: {
  optionKey: string;
  onChange: (newKey: string) => void;
  placeholder?: string;
}) {
  // For new options with placeholder keys like "option-123", show empty string
  const displayValue = optionKey.startsWith("option-") ? "" : optionKey;
  const [localValue, setLocalValue] = useState(displayValue);

  // Sync when external key changes
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

  // Base URL
  baseUrl: string;
  onBaseUrlChange: (value: string) => void;

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
  baseUrl,
  onBaseUrlChange,
  models,
  onModelsChange,
  extraOptions,
  onExtraOptionsChange,
}: OpenCodeFormFieldsProps) {
  const { t } = useTranslation();

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

  // Extra Options handlers
  const handleAddExtraOption = () => {
    const newKey = `option-${Date.now()}`;
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

      {/* Extra Options Editor */}
      <div className="space-y-3">
        <div className="flex items-center justify-between">
          <FormLabel>
            {t("opencode.extraOptions", { defaultValue: "额外选项" })}
          </FormLabel>
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={handleAddExtraOption}
            className="h-7 gap-1"
          >
            <Plus className="h-3.5 w-3.5" />
            {t("opencode.addExtraOption", { defaultValue: "添加" })}
          </Button>
        </div>

        {Object.keys(extraOptions).length === 0 ? (
          <p className="text-sm text-muted-foreground py-2">
            {t("opencode.noExtraOptions", {
              defaultValue: "暂无额外选项",
            })}
          </p>
        ) : (
          <div className="space-y-2">
            <div className="flex items-center gap-2 text-xs text-muted-foreground px-1 mb-1">
              <span className="flex-1">
                {t("opencode.extraOptionKey", { defaultValue: "键名" })}
              </span>
              <span className="flex-1">
                {t("opencode.extraOptionValue", { defaultValue: "值" })}
              </span>
              <span className="w-9" />
            </div>
            {Object.entries(extraOptions).map(([key, value]) => (
              <div key={key} className="flex items-center gap-2">
                <ExtraOptionKeyInput
                  optionKey={key}
                  onChange={(newKey) => handleExtraOptionKeyChange(key, newKey)}
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

        <p className="text-xs text-muted-foreground">
          {t("opencode.extraOptionsHint", {
            defaultValue:
              "配置额外的 SDK 选项，如 timeout、setCacheKey 等。值会自动解析类型（数字、布尔值等）。",
          })}
        </p>
      </div>

      {/* Models Editor */}
      <div className="space-y-3">
        <div className="flex items-center justify-between">
          <FormLabel>
            {t("opencode.models", { defaultValue: "Models" })}
          </FormLabel>
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
                    className="h-9 w-9 shrink-0"
                  >
                    <ChevronRight
                      className={cn(
                        "h-4 w-4 transition-transform",
                        expandedModels.has(key) && "rotate-90",
                      )}
                    />
                  </Button>
                  <ModelIdInput
                    modelId={key}
                    onChange={(newId) => handleModelIdChange(key, newId)}
                    placeholder={t("opencode.modelId", {
                      defaultValue: "Model ID",
                    })}
                  />
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

                {/* Expanded model options */}
                {expandedModels.has(key) && (
                  <div className="ml-9 pl-4 border-l-2 border-muted space-y-2">
                    {Object.keys(model.options || {}).length === 0 ? (
                      <div className="flex items-center justify-between">
                        <p className="text-xs text-muted-foreground py-1">
                          {t("opencode.noModelOptions", {
                            defaultValue: "模型选项，点击 + 添加",
                          })}
                        </p>
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
                    ) : (
                      <>
                        {Object.entries(model.options || {}).map(
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
                        )}
                        <div className="flex items-center justify-end">
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
                      </>
                    )}
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
