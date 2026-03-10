import { useState, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { FormLabel } from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import { Plus, Trash2, ChevronRight } from "lucide-react";
import { ApiKeySection, EndpointField, RemoteModelSelector } from "./shared";
import { cn } from "@/lib/utils";
import type {
  ProviderCategory,
  IIAgentModelInfo,
  ProviderProxyConfig,
} from "@/types";

/**
 * Model ID input with remote picker.
 */
function ModelIdInput({
  modelId,
  onChange,
  baseUrl,
  apiKey,
  proxyConfig,
  placeholder,
}: {
  modelId: string;
  onChange: (newId: string) => void;
  baseUrl: string;
  apiKey: string;
  proxyConfig?: ProviderProxyConfig;
  placeholder?: string;
}) {
  return (
    <RemoteModelSelector
      id={`iiagent-model-id-${modelId}`}
      label=""
      value={modelId}
      onChange={onChange}
      baseUrl={baseUrl}
      apiKey={apiKey}
      apiFormat="openai_chat"
      proxyConfig={proxyConfig}
      placeholder={placeholder}
      className="flex-1 space-y-0"
    />
  );
}

/**
 * Model option key input with local state.
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

interface IIAgentFormFieldsProps {
  apiKey: string;
  onApiKeyChange: (value: string) => void;
  category?: ProviderCategory;
  shouldShowApiKeyLink: boolean;
  websiteUrl: string;
  isPartner?: boolean;
  partnerPromotionKey?: string;
  baseUrl: string;
  onBaseUrlChange: (value: string) => void;
  models: Record<string, IIAgentModelInfo>;
  onModelsChange: (models: Record<string, IIAgentModelInfo>) => void;

  // Provider-level proxy config for model enumeration
  proxyConfig?: ProviderProxyConfig;
}

export function IIAgentFormFields({
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
  proxyConfig,
}: IIAgentFormFieldsProps) {
  const { t } = useTranslation();
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
      [newKey]: {
        id: newKey,
        name: "",
        provider: "",
        description: "",
        context_window: 128000,
        max_output_tokens: 4096,
      },
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
    const newModels: Record<string, IIAgentModelInfo> = {};
    for (const [k, v] of Object.entries(models)) {
      if (k === oldKey) {
        newModels[newKey] = { ...v, id: newKey };
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

  const handleModelFieldChange = (
    key: string,
    field: keyof IIAgentModelInfo,
    value: any,
  ) => {
    const newModels = { ...models };
    if (!newModels[key]) return;

    let finalValue = value;
    if (field === "context_window" || field === "max_output_tokens") {
      finalValue = parseInt(value, 10);
      if (isNaN(finalValue)) finalValue = 0;
    }

    newModels[key] = { ...newModels[key], [field]: finalValue };
    onModelsChange(newModels);
  };

  const handleAddModelOption = (modelKey: string) => {
    const model = models[modelKey];
    if (!model) return;
    const optKey = `option-${Date.now()}`;
    const newOptions = { ...model.options, [optKey]: "" };
    handleModelFieldChange(modelKey, "options", newOptions);
  };

  const handleRemoveModelOption = (modelKey: string, optKey: string) => {
    const model = models[modelKey];
    if (!model || !model.options) return;
    const newOptions = { ...model.options };
    delete newOptions[optKey];
    handleModelFieldChange(modelKey, "options", newOptions);
  };

  const handleModelOptionKeyChange = (
    modelKey: string,
    oldOptKey: string,
    newOptKey: string,
  ) => {
    const model = models[modelKey];
    if (
      !model ||
      !model.options ||
      oldOptKey === newOptKey ||
      !newOptKey.trim()
    )
      return;
    const newOptions: Record<string, any> = {};
    for (const [k, v] of Object.entries(model.options)) {
      if (k === oldOptKey) newOptions[newOptKey] = v;
      else newOptions[k] = v;
    }
    handleModelFieldChange(modelKey, "options", newOptions);
  };

  const handleModelOptionValueChange = (
    modelKey: string,
    optKey: string,
    value: string,
  ) => {
    const model = models[modelKey];
    if (!model || !model.options) return;
    let finalValue: any = value;
    try {
      if (
        (value.startsWith("{") && value.endsWith("}")) ||
        (value.startsWith("[") && value.endsWith("]"))
      ) {
        finalValue = JSON.parse(value);
      } else if (value === "true") finalValue = true;
      else if (value === "false") finalValue = false;
      else if (!isNaN(Number(value)) && value.trim() !== "")
        finalValue = Number(value);
    } catch {
      finalValue = value;
    }
    const newOptions = { ...model.options, [optKey]: finalValue };
    handleModelFieldChange(modelKey, "options", newOptions);
  };

  return (
    <div className="space-y-6">
      <ApiKeySection
        value={apiKey}
        onChange={onApiKeyChange}
        category={category}
        shouldShowLink={shouldShowApiKeyLink}
        websiteUrl={websiteUrl}
        isPartner={isPartner}
        partnerPromotionKey={partnerPromotionKey}
      />

      <EndpointField
        id="iiagent-baseurl"
        label={t("iiagent.baseUrl", { defaultValue: "API Endpoint" })}
        value={baseUrl}
        onChange={onBaseUrlChange}
        placeholder="https://api.anthropic.com"
        hint={t("iiagent.baseUrlHint", {
          defaultValue: "The base URL for IIAgent requests.",
        })}
      />

      <div className="space-y-3">
        <div className="flex items-center justify-between">
          <FormLabel className="text-base font-semibold">
            {t("iiagent.models", { defaultValue: "Models" })}
          </FormLabel>
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={handleAddModel}
            className="h-7 gap-1"
          >
            <Plus className="h-3.5 w-3.5" />
            {t("iiagent.addModel", { defaultValue: "Add" })}
          </Button>
        </div>

        {Object.keys(models).length === 0 ? (
          <p className="text-sm text-muted-foreground py-2">
            {t("iiagent.noModels", {
              defaultValue: "No models configured. Click Add to add a model.",
            })}
          </p>
        ) : (
          <div className="space-y-3">
            {Object.entries(models).map(([key, model]) => (
              <div
                key={key}
                className="space-y-2 p-3 bg-muted/30 rounded-lg border border-border"
              >
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
                    baseUrl={baseUrl}
                    apiKey={apiKey}
                    proxyConfig={proxyConfig}
                    placeholder={t("iiagent.modelId", {
                      defaultValue: "Model ID",
                    })}
                  />
                  <Input
                    value={model.name}
                    onChange={(e) =>
                      handleModelFieldChange(key, "name", e.target.value)
                    }
                    placeholder={t("iiagent.modelName", {
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
                  <div className="ml-9 pl-4 border-l-2 border-muted space-y-4 pt-2">
                    <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
                      <div className="space-y-1.5">
                        <FormLabel className="text-xs">
                          {t("iiagent.provider", { defaultValue: "Provider" })}
                        </FormLabel>
                        <Input
                          value={model.provider}
                          onChange={(e) =>
                            handleModelFieldChange(
                              key,
                              "provider",
                              e.target.value,
                            )
                          }
                          placeholder="Anthropic"
                          className="h-8 text-sm"
                        />
                      </div>
                      <div className="space-y-1.5">
                        <FormLabel className="text-xs">
                          {t("iiagent.description", {
                            defaultValue: "Description",
                          })}
                        </FormLabel>
                        <Input
                          value={model.description}
                          onChange={(e) =>
                            handleModelFieldChange(
                              key,
                              "description",
                              e.target.value,
                            )
                          }
                          placeholder="Optional description"
                          className="h-8 text-sm"
                        />
                      </div>
                    </div>

                    <div className="grid grid-cols-2 gap-3">
                      <div className="space-y-1.5">
                        <FormLabel className="text-xs">
                          {t("iiagent.contextWindow", {
                            defaultValue: "Context",
                          })}
                        </FormLabel>
                        <Input
                          type="number"
                          value={model.context_window}
                          onChange={(e) =>
                            handleModelFieldChange(
                              key,
                              "context_window",
                              e.target.value,
                            )
                          }
                          className="h-8 text-sm"
                        />
                      </div>
                      <div className="space-y-1.5">
                        <FormLabel className="text-xs">
                          {t("iiagent.maxOutputTokens", {
                            defaultValue: "Max Output",
                          })}
                        </FormLabel>
                        <Input
                          type="number"
                          value={model.max_output_tokens}
                          onChange={(e) =>
                            handleModelFieldChange(
                              key,
                              "max_output_tokens",
                              e.target.value,
                            )
                          }
                          className="h-8 text-sm"
                        />
                      </div>
                    </div>

                    {/* Model Options */}
                    <div className="space-y-2">
                      <div className="flex items-center justify-between">
                        <FormLabel className="text-xs font-medium">
                          {t("iiagent.modelOptions", {
                            defaultValue: "Extra Options",
                          })}
                        </FormLabel>
                        <Button
                          type="button"
                          variant="ghost"
                          size="sm"
                          onClick={() => handleAddModelOption(key)}
                          className="h-6 px-2 gap-1 text-xs"
                        >
                          <Plus className="h-3 w-3" />
                          {t("iiagent.addOption", {
                            defaultValue: "Add Option",
                          })}
                        </Button>
                      </div>

                      {!model.options ||
                        Object.keys(model.options).length === 0 ? (
                        <p className="text-[10px] text-muted-foreground italic">
                          {t("iiagent.noOptions", {
                            defaultValue: "No extra options configured",
                          })}
                        </p>
                      ) : (
                        <div className="space-y-2">
                          {Object.entries(model.options).map(
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
                                  placeholder="key (e.g. provider)"
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
                                  placeholder='value (e.g. "baseten")'
                                  className="flex-1 h-8 text-sm"
                                />
                                <Button
                                  type="button"
                                  variant="ghost"
                                  size="icon"
                                  onClick={() =>
                                    handleRemoveModelOption(key, optKey)
                                  }
                                  className="h-8 w-8 text-muted-foreground hover:text-destructive"
                                >
                                  <Trash2 className="h-3.5 w-3.5" />
                                </Button>
                              </div>
                            ),
                          )}
                        </div>
                      )}
                    </div>
                  </div>
                )}
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
