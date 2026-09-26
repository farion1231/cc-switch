import { useTranslation } from "react-i18next";
import { useState, useRef, useCallback } from "react";
import { FormLabel } from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import { ImeSafeInput } from "@/components/ui/ime-safe-input";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
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
import { stepcodeApiProtocols } from "@/config/stepcodeProviderPresets";
import type { ProviderCategory, StepCodeModel } from "@/types";

interface StepcodeFormFieldsProps {
  // Base URL
  baseUrl: string;
  onBaseUrlChange: (value: string) => void;

  // API Key
  apiKey: string;
  onApiKeyChange: (value: string) => void;
  category?: ProviderCategory;
  shouldShowApiKeyLink: boolean;
  websiteUrl: string;
  isPartner?: boolean;
  partnerPromotionKey?: string;

  // API Protocol
  api: string;
  onApiChange: (value: string) => void;

  // Models
  models: StepCodeModel[];
  onModelsChange: (models: StepCodeModel[]) => void;
}

export function StepcodeFormFields({
  baseUrl,
  onBaseUrlChange,
  apiKey,
  onApiKeyChange,
  category,
  shouldShowApiKeyLink,
  websiteUrl,
  isPartner,
  partnerPromotionKey,
  api,
  onApiChange,
  models,
  onModelsChange,
}: StepcodeFormFieldsProps) {
  const { t } = useTranslation();
  const [expandedModels, setExpandedModels] = useState<Set<string>>(new Set());
  const [fetchedModels, setFetchedModels] = useState<FetchedModel[]>([]);
  const [isFetchingModels, setIsFetchingModels] = useState(false);

  // Stable key tracking for models list
  const modelKeysRef = useRef<string[]>([]);
  const getModelKeys = useCallback(() => {
    // Grow keys array if models were added externally
    while (modelKeysRef.current.length < models.length) {
      modelKeysRef.current.push(crypto.randomUUID());
    }
    // Shrink if models were removed externally
    if (modelKeysRef.current.length > models.length) {
      modelKeysRef.current.length = models.length;
    }
    return modelKeysRef.current;
  }, [models.length]);
  const modelKeys = getModelKeys();

  // Toggle advanced section for a model
  const toggleModelAdvanced = (modelKey: string) => {
    setExpandedModels((prev) => {
      const next = new Set(prev);
      if (next.has(modelKey)) {
        next.delete(modelKey);
      } else {
        next.add(modelKey);
      }
      return next;
    });
  };

  // Add a new model entry
  const handleAddModel = () => {
    modelKeysRef.current.push(crypto.randomUUID());
    onModelsChange([
      ...models,
      {
        id: "",
        name: "",
        contextWindow: undefined,
        maxTokens: undefined,
      },
    ]);
  };

  // Fetch models from API
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

  // Remove a model entry
  const handleRemoveModel = (index: number) => {
    const removedKey = modelKeysRef.current[index];
    modelKeysRef.current.splice(index, 1);
    const newModels = [...models];
    newModels.splice(index, 1);
    onModelsChange(newModels);
    if (removedKey) {
      setExpandedModels((prev) => {
        const next = new Set(prev);
        next.delete(removedKey);
        return next;
      });
    }
  };

  // Update model field
  const handleModelChange = (
    index: number,
    field: keyof StepCodeModel,
    value: unknown,
  ) => {
    const newModels = [...models];
    newModels[index] = { ...newModels[index], [field]: value };
    onModelsChange(newModels);
  };

  return (
    <>
      {/* API Protocol Selector */}
      <div className="space-y-2">
        <FormLabel htmlFor="stepcode-api">
          {t("stepcode.apiProtocol", {
            defaultValue: "API Protocol",
          })}
        </FormLabel>
        <Select value={api} onValueChange={onApiChange}>
          <SelectTrigger id="stepcode-api">
            <SelectValue
              placeholder={t("stepcode.selectProtocol", {
                defaultValue: "Select API Protocol",
              })}
            />
          </SelectTrigger>
          <SelectContent>
            {stepcodeApiProtocols.map((protocol) => (
              <SelectItem key={protocol.value} value={protocol.value}>
                {protocol.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <p className="text-xs text-muted-foreground">
          {t("stepcode.apiProtocolHint", {
            defaultValue:
              "Select the protocol type compatible with the provider's API. Most providers use OpenAI Completions format.",
          })}
        </p>
      </div>

      {/* Base URL */}
      <div className="space-y-2">
        <FormLabel htmlFor="stepcode-baseurl">
          {t("stepcode.baseUrl", { defaultValue: "API Endpoint" })}
        </FormLabel>
        <ImeSafeInput
          id="stepcode-baseurl"
          value={baseUrl}
          onValueChange={onBaseUrlChange}
          placeholder="https://api.example.com/v1"
        />
        <p className="text-xs text-muted-foreground">
          {t("stepcode.baseUrlHint", {
            defaultValue: "The provider's API endpoint address.",
          })}
        </p>
      </div>

      {/* API Key */}
      <ApiKeySection
        value={apiKey}
        onChange={onApiKeyChange}
        // StepCode 的 API key 始终由用户自填，没有 OAuth-only 的免 key 官方供应商，
        // 故不让 official 禁用输入框（与 OpenClaw / Hermes 对齐）。
        category={category === "official" ? undefined : category}
        shouldShowLink={shouldShowApiKeyLink}
        websiteUrl={websiteUrl}
        isPartner={isPartner}
        partnerPromotionKey={partnerPromotionKey}
      />

      {/* Models Editor */}
      <div className="space-y-3 border-l border-border-default pl-3">
        <div className="flex items-center justify-between gap-3">
          <FormLabel>
            {t("stepcode.models", { defaultValue: "Model Configuration" })}
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
              {t("stepcode.addModel", { defaultValue: "Add Model" })}
            </Button>
          </div>
        </div>

        {models.length === 0 ? (
          <p role="status" className="py-2 text-sm text-muted-foreground">
            {t("stepcode.noModels", {
              defaultValue: "No models configured",
            })}
          </p>
        ) : (
          <div className="space-y-2">
            <div className="flex items-center gap-2 px-1 text-xs text-muted-foreground">
              <span className="w-9" />
              <span className="flex-1">
                {t("stepcode.modelId", { defaultValue: "Model ID" })}
              </span>
              <span className="flex-1">
                {t("stepcode.modelName", { defaultValue: "Display Name" })}
              </span>
              <span className="w-9" />
            </div>
            {models.map((model, index) => {
              const modelKey = modelKeys[index];
              const isExpanded = expandedModels.has(modelKey);
              return (
                <div key={modelKey} className="space-y-2">
                  <div className="flex items-center gap-2">
                    <Button
                      type="button"
                      variant="ghost"
                      size="icon"
                      onClick={() => toggleModelAdvanced(modelKey)}
                      aria-expanded={isExpanded}
                      aria-label={t("stepcode.toggleModelDetails", {
                        defaultValue: "Expand or collapse model details",
                      })}
                      className="h-9 w-9 shrink-0"
                    >
                      <ChevronRight
                        className={`h-4 w-4 transition-transform motion-reduce:transition-none ${
                          isExpanded ? "rotate-90" : ""
                        }`}
                      />
                    </Button>
                    <div className="flex min-w-0 flex-1 gap-1">
                      <ImeSafeInput
                        value={model.id}
                        onValueChange={(value) =>
                          handleModelChange(index, "id", value)
                        }
                        placeholder={t("stepcode.modelIdPlaceholder", {
                          defaultValue: "claude-3-sonnet",
                        })}
                        aria-label={t("stepcode.modelId", {
                          defaultValue: "Model ID",
                        })}
                        className="min-w-0 flex-1"
                      />
                      {fetchedModels.length > 0 && (
                        <ModelDropdown
                          models={fetchedModels}
                          onSelect={(id) => handleModelChange(index, "id", id)}
                        />
                      )}
                    </div>
                    <ImeSafeInput
                      value={model.name ?? ""}
                      onValueChange={(value) =>
                        handleModelChange(index, "name", value)
                      }
                      placeholder={t("stepcode.modelNamePlaceholder", {
                        defaultValue: "Claude 3 Sonnet",
                      })}
                      aria-label={t("stepcode.modelName", {
                        defaultValue: "Display Name",
                      })}
                      className="min-w-0 flex-1"
                    />
                    <Button
                      type="button"
                      variant="ghost"
                      size="icon"
                      onClick={() => handleRemoveModel(index)}
                      aria-label={t("stepcode.removeModel", {
                        defaultValue: "Remove model",
                      })}
                      className="h-9 w-9 shrink-0 text-muted-foreground hover:text-destructive"
                    >
                      <Trash2 className="h-4 w-4" />
                    </Button>
                  </div>

                  {isExpanded && (
                    <div className="ml-9 grid gap-3 border-l-2 border-muted pl-4 sm:grid-cols-2 animate-in fade-in-0 slide-in-from-top-1 duration-200 motion-reduce:animate-none">
                      <div className="flex min-h-9 items-center gap-2.5 sm:col-span-2">
                        <FormLabel
                          htmlFor={`stepcode-model-reasoning-${modelKey}`}
                          className="cursor-pointer"
                        >
                          {t("stepcode.reasoning", {
                            defaultValue: "Supports Extended Thinking",
                          })}
                        </FormLabel>
                        <Switch
                          id={`stepcode-model-reasoning-${modelKey}`}
                          checked={model.reasoning ?? false}
                          onCheckedChange={(checked) =>
                            handleModelChange(index, "reasoning", checked)
                          }
                        />
                      </div>

                      <div className="space-y-1">
                        <FormLabel
                          htmlFor={`stepcode-model-context-${modelKey}`}
                          className="text-xs text-muted-foreground"
                        >
                          {t("stepcode.contextWindow", {
                            defaultValue: "Context Length",
                          })}
                        </FormLabel>
                        <Input
                          id={`stepcode-model-context-${modelKey}`}
                          type="number"
                          min={1}
                          step={1}
                          value={model.contextWindow ?? ""}
                          onChange={(event) =>
                            handleModelChange(
                              index,
                              "contextWindow",
                              event.target.value
                                ? parseInt(event.target.value)
                                : undefined,
                            )
                          }
                          placeholder="200000"
                        />
                      </div>
                      <div className="space-y-1">
                        <FormLabel
                          htmlFor={`stepcode-model-max-tokens-${modelKey}`}
                          className="text-xs text-muted-foreground"
                        >
                          {t("stepcode.maxTokens", {
                            defaultValue: "Max Output Tokens",
                          })}
                        </FormLabel>
                        <Input
                          id={`stepcode-model-max-tokens-${modelKey}`}
                          type="number"
                          min={1}
                          step={1}
                          value={model.maxTokens ?? ""}
                          onChange={(event) =>
                            handleModelChange(
                              index,
                              "maxTokens",
                              event.target.value
                                ? parseInt(event.target.value)
                                : undefined,
                            )
                          }
                          placeholder="32000"
                        />
                      </div>
                    </div>
                  )}
                </div>
              );
            })}
          </div>
        )}
      </div>
    </>
  );
}
