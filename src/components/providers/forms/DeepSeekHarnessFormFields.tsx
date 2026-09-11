import { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";
import { Download, Loader2, Plus, Trash2 } from "lucide-react";
import { toast } from "sonner";
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
import { ApiKeySection, ModelDropdown } from "./shared";
import {
  fetchModelsForConfig,
  showFetchModelsError,
  type FetchedModel,
} from "@/lib/api/model-fetch";
import {
  DSH_MODEL_API_OPTIONS,
  type DshModel,
} from "./helpers/deepseekHarnessFormUtils";
import type { ProviderCategory } from "@/types";

const API_FORMAT_LABELS: Record<string, string> = {
  "openai-completions": "OpenAI Chat Completions",
  "openai-responses": "OpenAI Responses",
  "anthropic-messages": "Anthropic Messages",
};

interface DeepSeekHarnessFormFieldsProps {
  isOfficial: boolean;

  // API Key
  apiKey: string;
  onApiKeyChange: (value: string) => void;
  apiKeyEnv: string;
  onApiKeyEnvChange: (value: string) => void;
  category?: ProviderCategory;
  shouldShowApiKeyLink: boolean;
  websiteUrl: string;
  isPartner?: boolean;
  partnerPromotionKey?: string;

  // Base URL
  baseUrl: string;
  onBaseUrlChange: (value: string) => void;

  // API format
  api: string;
  onApiChange: (value: string) => void;

  // Models
  models: DshModel[];
  onModelsChange: (models: DshModel[]) => void;

  // Default model
  defaultModel: string;
  onDefaultModelChange: (value: string) => void;
}

export function DeepSeekHarnessFormFields({
  isOfficial,
  apiKey,
  onApiKeyChange,
  apiKeyEnv,
  onApiKeyEnvChange,
  category,
  shouldShowApiKeyLink,
  websiteUrl,
  isPartner,
  partnerPromotionKey,
  baseUrl,
  onBaseUrlChange,
  api,
  onApiChange,
  models,
  onModelsChange,
  defaultModel,
  onDefaultModelChange,
}: DeepSeekHarnessFormFieldsProps) {
  const { t } = useTranslation();

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

  const updateModel = (index: number, patch: Partial<DshModel>) => {
    onModelsChange(
      models.map((model, i) => (i === index ? { ...model, ...patch } : model)),
    );
  };

  const removeModel = (index: number) => {
    onModelsChange(models.filter((_, i) => i !== index));
  };

  const addModel = (id = "") => {
    const trimmed = id.trim();
    if (trimmed && models.some((model) => model.id === trimmed)) return;
    onModelsChange([...models, { id: trimmed, name: trimmed || undefined }]);
  };

  return (
    <>
      {/* API Key */}
      <ApiKeySection
        id="dsh-api-key"
        value={apiKey}
        onChange={onApiKeyChange}
        category={category}
        shouldShowLink={shouldShowApiKeyLink}
        websiteUrl={websiteUrl}
        isPartner={isPartner}
        partnerPromotionKey={partnerPromotionKey}
      />

      {/* Credential Ref (custom routes only) */}
      {!isOfficial && (
        <div className="space-y-2">
          <FormLabel htmlFor="dsh-credential-ref">
            {t("deepseekHarness.credentialRef", {
              defaultValue: "Credential Ref",
            })}
          </FormLabel>
          <ImeSafeInput
            id="dsh-credential-ref"
            value={apiKeyEnv}
            onValueChange={(value) => onApiKeyEnvChange(value.toUpperCase())}
            normalize={(value) => value.toUpperCase()}
            placeholder="CUSTOM_DSH_API_KEY"
            autoComplete="off"
          />
          <p className="text-xs text-muted-foreground">
            {t("deepseekHarness.credentialRefHint", {
              defaultValue:
                "Uppercase identifier used to store the API key in DSH credentials.",
            })}
          </p>
        </div>
      )}

      {/* Base URL */}
      <div className="space-y-2">
        <FormLabel htmlFor="dsh-base-url">
          {t("deepseekHarness.baseUrl", { defaultValue: "Base URL" })}
        </FormLabel>
        <ImeSafeInput
          id="dsh-base-url"
          value={baseUrl}
          onValueChange={onBaseUrlChange}
          placeholder="https://api.example.com/v1"
        />
        <p className="text-xs text-muted-foreground">
          {t("deepseekHarness.baseUrlHint", {
            defaultValue: "The base URL for the API endpoint.",
          })}
        </p>
      </div>

      {/* API Format (custom routes only) */}
      {!isOfficial && (
        <div className="space-y-2">
          <FormLabel htmlFor="dsh-api-format">
            {t("deepseekHarness.apiFormat", { defaultValue: "API Format" })}
          </FormLabel>
          <Select value={api} onValueChange={onApiChange}>
            <SelectTrigger id="dsh-api-format">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {DSH_MODEL_API_OPTIONS.map((option) => (
                <SelectItem key={option} value={option}>
                  {t(`deepseekHarness.api_${option.replace(/-/g, "_")}`, {
                    defaultValue: API_FORMAT_LABELS[option] ?? option,
                  })}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      )}

      {/* Model Catalog */}
      <div className="space-y-3 border-l border-border-default pl-3">
        <div className="flex items-center justify-between gap-2">
          <FormLabel>
            {t("deepseekHarness.modelCatalog", {
              defaultValue: "Model Catalog",
            })}
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
              {t("providerForm.fetchModels", { defaultValue: "Fetch Models" })}
            </Button>
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={() => addModel()}
              className="h-7 gap-1"
            >
              <Plus className="h-3.5 w-3.5" />
              {t("deepseekHarness.addModel", { defaultValue: "Add" })}
            </Button>
          </div>
        </div>

        {models.length === 0 ? (
          <p className="text-sm text-muted-foreground py-2">
            {t("deepseekHarness.noModels", {
              defaultValue: "No models configured. Click Add to add a model.",
            })}
          </p>
        ) : (
          <div className="space-y-2">
            <div className="flex items-center gap-2 text-xs text-muted-foreground px-1 mb-1">
              <span className="flex-1">
                {t("deepseekHarness.modelId", { defaultValue: "Model ID" })}
              </span>
              <span className="flex-1">
                {t("deepseekHarness.modelName", {
                  defaultValue: "Display Name",
                })}
              </span>
              <span className="w-32">
                {t("deepseekHarness.contextWindow", {
                  defaultValue: "Context Window",
                })}
              </span>
              <span className="w-9" />
            </div>
            {models.map((model, index) => (
              <div key={index} className="flex items-center gap-2">
                <div className="flex min-w-0 flex-1 gap-1">
                  <ImeSafeInput
                    value={model.id}
                    onValueChange={(value) => updateModel(index, { id: value })}
                    placeholder={t("deepseekHarness.modelId", {
                      defaultValue: "Model ID",
                    })}
                    className="flex-1"
                  />
                  {fetchedModels.length > 0 && (
                    <ModelDropdown
                      models={fetchedModels}
                      onSelect={(id) => updateModel(index, { id })}
                    />
                  )}
                </div>
                <ImeSafeInput
                  value={model.name ?? ""}
                  onValueChange={(value) => updateModel(index, { name: value })}
                  placeholder={t("deepseekHarness.modelName", {
                    defaultValue: "Display Name",
                  })}
                  className="flex-1"
                />
                <Input
                  type="number"
                  min={0}
                  step={1}
                  aria-label={t("deepseekHarness.contextWindow", {
                    defaultValue: "Context Window",
                  })}
                  value={model.contextWindow ?? ""}
                  onChange={(event) => {
                    const raw = event.target.value.trim();
                    if (raw === "") {
                      updateModel(index, { contextWindow: undefined });
                      return;
                    }
                    const parsed = Number(raw);
                    if (Number.isFinite(parsed) && parsed >= 0) {
                      updateModel(index, { contextWindow: Math.trunc(parsed) });
                    }
                  }}
                  placeholder="131072"
                  className="w-32"
                />
                <Button
                  type="button"
                  variant="ghost"
                  size="icon"
                  onClick={() => removeModel(index)}
                  aria-label={t("common.remove", { defaultValue: "Remove" })}
                  className="h-9 w-9 text-muted-foreground hover:text-destructive"
                >
                  <Trash2 className="h-4 w-4" />
                </Button>
              </div>
            ))}
          </div>
        )}

        {/* Default Model */}
        <div className="space-y-2">
          <FormLabel htmlFor="dsh-default-model">
            {t("deepseekHarness.defaultModel", {
              defaultValue: "Default Model",
            })}
          </FormLabel>
          <ImeSafeInput
            id="dsh-default-model"
            value={defaultModel}
            onValueChange={onDefaultModelChange}
            placeholder={models[0]?.id ?? ""}
          />
          <p className="text-xs text-muted-foreground">
            {t("deepseekHarness.defaultModelHint", {
              defaultValue: "The model DSH uses by default.",
            })}
          </p>
        </div>
      </div>
    </>
  );
}
