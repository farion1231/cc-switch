import { useTranslation } from "react-i18next";
import { useState, useRef, useCallback } from "react";
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
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import { toast } from "sonner";
import {
  Download,
  Plus,
  Trash2,
  ChevronDown,
  ChevronRight,
  Loader2,
} from "lucide-react";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { ApiKeySection } from "./shared";
import {
  fetchModelsForConfig,
  showFetchModelsError,
  type FetchedModel,
} from "@/lib/api/model-fetch";
import type { ProviderCategory, PiModelEntry } from "@/types";

const PI_API_PROTOCOLS = [
  "openai-completions",
  "openai-responses",
  "anthropic-messages",
  "google-generative-ai",
] as const;

interface PiFormFieldsProps {
  baseUrl: string;
  onBaseUrlChange: (value: string) => void;
  apiKey: string;
  onApiKeyChange: (value: string) => void;
  category?: ProviderCategory;
  shouldShowApiKeyLink: boolean;
  websiteUrl: string;
  isPartner?: boolean;
  partnerPromotionKey?: string;
  api: string;
  onApiChange: (value: string) => void;
  models: PiModelEntry[];
  onModelsChange: (models: PiModelEntry[]) => void;
}

export function PiFormFields({
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
}: PiFormFieldsProps) {
  const { t } = useTranslation();
  const [expandedModels, setExpandedModels] = useState<Record<number, boolean>>(
    {},
  );
  const [fetchedModels, setFetchedModels] = useState<FetchedModel[]>([]);
  const [isFetchingModels, setIsFetchingModels] = useState(false);

  const modelKeysRef = useRef<string[]>([]);
  const getModelKeys = useCallback(() => {
    while (modelKeysRef.current.length < models.length) {
      modelKeysRef.current.push(crypto.randomUUID());
    }
    if (modelKeysRef.current.length > models.length) {
      modelKeysRef.current.length = models.length;
    }
    return modelKeysRef.current;
  }, [models.length]);
  const modelKeys = getModelKeys();

  const toggleModelAdvanced = (index: number) => {
    setExpandedModels((prev) => ({ ...prev, [index]: !prev[index] }));
  };

  const handleAddModel = () => {
    modelKeysRef.current.push(crypto.randomUUID());
    onModelsChange([
      ...models,
      {
        id: "",
        name: "",
        reasoning: false,
        input: ["text"],
      },
    ]);
  };

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
        } else {
          toast.success(
            t("providerForm.fetchModelsSuccess", { count: list.length }),
          );
        }
      })
      .catch((err) => {
        console.warn("[ModelFetch] Failed:", err);
        showFetchModelsError(err, t);
      })
      .finally(() => setIsFetchingModels(false));
  }, [baseUrl, apiKey, t]);

  const handleRemoveModel = (index: number) => {
    modelKeysRef.current.splice(index, 1);
    const next = [...models];
    next.splice(index, 1);
    onModelsChange(next);
  };

  const handleModelFieldChange = (
    index: number,
    field: keyof PiModelEntry,
    value: unknown,
  ) => {
    const next = [...models];
    next[index] = { ...next[index], [field]: value };
    onModelsChange(next);
  };

  const handleAddFetchedModel = (model: FetchedModel) => {
    if (models.some((m) => m.id === model.id)) {
      toast.info(
        t("providerForm.modelAlreadyAdded", {
          defaultValue: "Model already added",
        }),
      );
      return;
    }
    modelKeysRef.current.push(crypto.randomUUID());
    onModelsChange([
      ...models,
      {
        id: model.id,
        name: model.id,
        reasoning: false,
        input: ["text"],
      },
    ]);
  };

  return (
    <div className="space-y-4">
      <div className="space-y-2">
        <FormLabel>
          {t("pi.form.baseUrl", { defaultValue: "Base URL" })}
        </FormLabel>
        <Input
          value={baseUrl}
          onChange={(e) => onBaseUrlChange(e.target.value)}
          placeholder="https://api.openai.com/v1"
        />
      </div>

      <ApiKeySection
        value={apiKey}
        onChange={onApiKeyChange}
        // Pi 没有 OAuth-only 的免 key 官方供应商：即便是 official 预设
        // （OpenAI / Anthropic / Gemini）也需用户自填 key，故不让 official 禁用输入框。
        category={category === "official" ? undefined : category}
        shouldShowLink={shouldShowApiKeyLink}
        websiteUrl={websiteUrl}
        isPartner={isPartner}
        partnerPromotionKey={partnerPromotionKey}
      />

      <div className="space-y-2">
        <FormLabel>
          {t("pi.form.api", { defaultValue: "API Protocol" })}
        </FormLabel>
        <Select value={api || "openai-completions"} onValueChange={onApiChange}>
          <SelectTrigger>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {PI_API_PROTOCOLS.map((protocol) => (
              <SelectItem key={protocol} value={protocol}>
                {protocol}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>

      <div className="space-y-3">
        <div className="flex items-center justify-between gap-2">
          <FormLabel>
            {t("pi.form.models", { defaultValue: "Models" })}
          </FormLabel>
          <div className="flex items-center gap-1">
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  disabled={isFetchingModels}
                >
                  {isFetchingModels ? (
                    <Loader2 className="h-4 w-4 animate-spin" />
                  ) : (
                    <Download className="h-4 w-4" />
                  )}
                  {t("providerForm.fetchModels", {
                    defaultValue: "Fetch",
                  })}
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end" className="max-h-72 overflow-auto">
                <DropdownMenuLabel>
                  {t("providerForm.fetchedModels", {
                    defaultValue: "Fetched models",
                  })}
                </DropdownMenuLabel>
                <DropdownMenuSeparator />
                {fetchedModels.length === 0 ? (
                  <DropdownMenuItem onSelect={handleFetchModels}>
                    {t("providerForm.fetchModelsHint", {
                      defaultValue: "Click to fetch models from API",
                    })}
                  </DropdownMenuItem>
                ) : (
                  fetchedModels.map((model) => (
                    <DropdownMenuItem
                      key={model.id}
                      onSelect={() => handleAddFetchedModel(model)}
                    >
                      {model.id}
                    </DropdownMenuItem>
                  ))
                )}
              </DropdownMenuContent>
            </DropdownMenu>
            <Button type="button" variant="outline" size="sm" onClick={handleAddModel}>
              <Plus className="h-4 w-4" />
              {t("common.add", { defaultValue: "Add" })}
            </Button>
          </div>
        </div>

        {models.length === 0 ? (
          <p className="text-xs text-muted-foreground">
            {t("pi.form.modelsEmpty", {
              defaultValue: "No models yet. Add or fetch models.",
            })}
          </p>
        ) : (
          <div className="space-y-2">
            {models.map((model, index) => (
              <div
                key={modelKeys[index]}
                className="rounded-lg border border-border/60 p-3 space-y-2"
              >
                <div className="flex items-start gap-2">
                  <div className="grid flex-1 gap-2 sm:grid-cols-2">
                    <Input
                      value={model.id || ""}
                      onChange={(e) =>
                        handleModelFieldChange(index, "id", e.target.value)
                      }
                      placeholder={t("pi.form.modelId", {
                        defaultValue: "Model ID",
                      })}
                    />
                    <Input
                      value={model.name || ""}
                      onChange={(e) =>
                        handleModelFieldChange(index, "name", e.target.value)
                      }
                      placeholder={t("pi.form.modelName", {
                        defaultValue: "Display name",
                      })}
                    />
                  </div>
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon"
                    onClick={() => handleRemoveModel(index)}
                    title={t("common.delete")}
                  >
                    <Trash2 className="h-4 w-4" />
                  </Button>
                </div>

                <Collapsible
                  open={!!expandedModels[index]}
                  onOpenChange={() => toggleModelAdvanced(index)}
                >
                  <CollapsibleTrigger asChild>
                    <Button
                      type="button"
                      variant="ghost"
                      size="sm"
                      className="h-7 px-2 text-xs text-muted-foreground"
                    >
                      {expandedModels[index] ? (
                        <ChevronDown className="h-3.5 w-3.5" />
                      ) : (
                        <ChevronRight className="h-3.5 w-3.5" />
                      )}
                      {t("pi.form.advanced", { defaultValue: "Advanced" })}
                    </Button>
                  </CollapsibleTrigger>
                  <CollapsibleContent className="space-y-2 pt-2">
                    <div className="grid gap-2 sm:grid-cols-2">
                      <Input
                        type="number"
                        value={model.contextWindow ?? ""}
                        onChange={(e) =>
                          handleModelFieldChange(
                            index,
                            "contextWindow",
                            e.target.value
                              ? Number(e.target.value)
                              : undefined,
                          )
                        }
                        placeholder={t("pi.form.contextWindow", {
                          defaultValue: "Context window",
                        })}
                      />
                      <Input
                        type="number"
                        value={model.maxTokens ?? ""}
                        onChange={(e) =>
                          handleModelFieldChange(
                            index,
                            "maxTokens",
                            e.target.value
                              ? Number(e.target.value)
                              : undefined,
                          )
                        }
                        placeholder={t("pi.form.maxTokens", {
                          defaultValue: "Max tokens",
                        })}
                      />
                    </div>
                    <label className="flex items-center gap-2 text-sm">
                      <input
                        type="checkbox"
                        checked={!!model.reasoning}
                        onChange={(e) =>
                          handleModelFieldChange(
                            index,
                            "reasoning",
                            e.target.checked,
                          )
                        }
                      />
                      {t("pi.form.reasoning", {
                        defaultValue: "Reasoning model",
                      })}
                    </label>
                  </CollapsibleContent>
                </Collapsible>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
