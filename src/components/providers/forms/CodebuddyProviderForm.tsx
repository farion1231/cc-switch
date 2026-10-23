import { useCallback, useEffect, useRef, useState } from "react";
import { useForm } from "react-hook-form";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Form } from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import JsonEditor from "@/components/JsonEditor";
import { useDarkMode } from "@/hooks/useDarkMode";
import { toast } from "@/lib/toast";
import {
  fetchModelsForConfig,
  showFetchModelsError,
  type FetchedModel,
} from "@/lib/api/model-fetch";
import { useProvidersQuery } from "@/lib/query/queries";
import type { ProviderFormData } from "@/lib/schemas/provider";
import {
  CODEBUDDY_API_FORMATS,
  codebuddyProviderPresets,
  type CodebuddyModelConfig,
} from "@/config/codebuddyProviderPresets";
import { BasicFormFields, REQUIRED_LABEL } from "./BasicFormFields";
import { ApiKeySection, EndpointField, ModelInputWithFetch } from "./shared";
import { ProviderPresetSelector } from "./ProviderPresetSelector";
import type { ProviderFormProps } from "./ProviderForm";

const PRESET_ENTRIES = codebuddyProviderPresets.map((preset, index) => ({
  id: String(index),
  preset,
}));

/** Radix Select items need a non-empty value; this stands for "no `api` field". */
const CHAT_COMPLETIONS = "chat-completions";

const EMPTY_CONFIG: CodebuddyModelConfig = {
  vendor: "",
  url: "",
  apiKey: "",
  supportsToolCall: true,
};

const isValidUrl = (value: string) => {
  const trimmed = value.trim();
  if (trimmed.includes("${")) return true;
  try {
    const url = new URL(trimmed);
    return (
      (url.protocol === "http:" || url.protocol === "https:") && !!url.host
    );
  } catch {
    return false;
  }
};

const isModelConfig = (value: unknown): value is CodebuddyModelConfig =>
  typeof value === "object" && value !== null && !Array.isArray(value);

/** `id` lives in the provider key; strip it from the editable JSON. */
const withoutId = (config: Record<string, unknown>): CodebuddyModelConfig => {
  const { id: _id, ...rest } = config;
  return rest;
};

const toPositiveInt = (value: string): number | undefined => {
  const parsed = Number.parseInt(value, 10);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : undefined;
};

const CAPABILITIES = [
  "supportsToolCall",
  "supportsImages",
  "supportsReasoning",
] as const;

/**
 * Where `GET /models` lives for a CodeBuddy `url`: Chat Completions takes the full
 * `/chat/completions` path and Responses takes the base URL (`/responses` optional).
 */
export const codebuddyModelsBase = (
  url: string,
  api: unknown,
): { baseUrl: string; isFullUrl: boolean } => {
  const trimmed = url.trim().replace(/\/+$/, "");
  const suffix =
    api === "openai-responses" ? /\/responses$/ : /\/chat\/completions$/;
  const stripped = trimmed.replace(suffix, "");
  return api !== "openai-responses" && stripped === trimmed
    ? { baseUrl: trimmed, isFullUrl: true }
    : { baseUrl: stripped, isFullUrl: false };
};

export function CodebuddyProviderForm({
  providerId,
  initialData,
  onSubmit,
  onCancel,
  submitLabel,
  showButtons = true,
  onSubmittingChange,
  onSubmitReadyChange,
}: ProviderFormProps) {
  const { t } = useTranslation();
  const isDarkMode = useDarkMode();
  const [config, setConfig] = useState<CodebuddyModelConfig>(
    initialData?.settingsConfig
      ? withoutId(initialData.settingsConfig)
      : EMPTY_CONFIG,
  );
  const [jsonText, setJsonText] = useState(JSON.stringify(config, null, 2));
  const [jsonValid, setJsonValid] = useState(true);
  const [presetId, setPresetId] = useState("custom");
  const preset =
    presetId === "custom"
      ? undefined
      : codebuddyProviderPresets[Number(presetId)];
  const category = initialData?.category ?? preset?.category ?? "custom";
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  // The key is the model `id` in models.json and the value of `/model`, so it
  // is fixed once the model exists.
  const isEdit = Boolean(initialData);
  const [modelId, setModelId] = useState(providerId ?? "");
  const { data: existing } = useProvidersQuery("codebuddy");
  const trimmedId = modelId.trim();
  const keyTaken =
    !isEdit &&
    Object.prototype.hasOwnProperty.call(existing?.providers ?? {}, trimmedId);
  const keyInvalid = !isEdit && modelId !== "" && trimmedId !== modelId;
  const form = useForm<ProviderFormData>({
    defaultValues: {
      name: initialData?.name ?? "",
      notes: initialData?.notes ?? "",
      websiteUrl: initialData?.websiteUrl ?? "",
      icon: initialData?.icon ?? "",
      iconColor: initialData?.iconColor ?? "",
      settingsConfig: jsonText,
    },
  });
  const name = form.watch("name");
  const url = config.url ?? "";
  const urlInvalid = url.trim() !== "" && !isValidUrl(url);
  const ready = Boolean(
    jsonValid &&
      name.trim() &&
      (isEdit || (trimmedId && !keyInvalid && !keyTaken)) &&
      url.trim() &&
      !urlInvalid,
  );
  useEffect(() => {
    onSubmitReadyChange?.(ready);
  }, [ready, onSubmitReadyChange]);

  const update = (next: CodebuddyModelConfig) => {
    setConfig(next);
    setJsonText(JSON.stringify(next, null, 2));
    setJsonValid(true);
  };
  const setField = (key: keyof CodebuddyModelConfig, value: unknown) => {
    const next = { ...config };
    if (value === undefined || value === "") delete next[key];
    else next[key] = value;
    update(next);
  };

  const choosePreset = (id: string) => {
    setPresetId(id);
    const selected =
      id === "custom" ? undefined : codebuddyProviderPresets[Number(id)];
    const next = selected?.settingsConfig ?? EMPTY_CONFIG;
    update({ ...next });
    setModelId(selected?.providerKey ?? "");
    form.reset({
      name: selected?.settingsConfig.name ?? selected?.name ?? "",
      notes: "",
      websiteUrl: selected?.websiteUrl ?? "",
      icon: selected?.icon ?? "",
      iconColor: selected?.iconColor ?? "",
      settingsConfig: JSON.stringify(next),
    });
  };

  const [fetchedModels, setFetchedModels] = useState<FetchedModel[]>([]);
  const [isFetchingModels, setIsFetchingModels] = useState(false);
  const fetchGeneration = useRef(0);
  const apiKey = config.apiKey ?? "";
  const handleFetchModels = useCallback(() => {
    const endpoint = url.trim();
    const key = apiKey.trim();
    if (!endpoint || !key) {
      showFetchModelsError(null, t, {
        hasApiKey: Boolean(key),
        hasBaseUrl: Boolean(endpoint),
      });
      return;
    }
    const { baseUrl, isFullUrl } = codebuddyModelsBase(endpoint, config.api);
    const generation = ++fetchGeneration.current;
    setFetchedModels([]);
    setIsFetchingModels(true);
    fetchModelsForConfig(baseUrl, key, isFullUrl)
      .then((models) => {
        if (generation !== fetchGeneration.current) return;
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
        if (generation !== fetchGeneration.current) return;
        console.warn("[ModelFetch] Failed:", err);
        showFetchModelsError(err, t);
      })
      .finally(() => {
        if (generation === fetchGeneration.current) setIsFetchingModels(false);
      });
  }, [url, apiKey, config.api, t]);

  return (
    <Form {...form}>
      <form
        id="provider-form"
        className="space-y-6"
        onSubmit={form.handleSubmit(async (identity) => {
          if (!ready || busy) return;
          setBusy(true);
          onSubmittingChange?.(true);
          setError("");
          try {
            const settings: CodebuddyModelConfig = {
              ...config,
              name: identity.name.trim(),
            };
            for (const key of ["vendor", "apiKey", "api"] as const) {
              if (typeof settings[key] === "string" && !settings[key]?.trim())
                delete settings[key];
            }
            settings.url = url.trim();
            await onSubmit({
              ...identity,
              name: identity.name.trim(),
              meta: initialData?.meta,
              providerKey: isEdit ? providerId : trimmedId,
              presetCategory: category,
              settingsConfig: JSON.stringify(settings),
            });
          } catch (error) {
            setError(String(error));
          } finally {
            setBusy(false);
            onSubmittingChange?.(false);
          }
        })}
      >
        {!initialData && (
          <ProviderPresetSelector
            selectedPresetId={presetId}
            presetEntries={PRESET_ENTRIES}
            presetCategoryLabels={{
              custom: t("providerPreset.custom"),
              cn_official: t("providerForm.categoryCnOfficial"),
              aggregator: t("providerForm.categoryAggregation"),
              third_party: t("providerForm.categoryThirdParty"),
            }}
            onPresetChange={choosePreset}
            category={category}
          />
        )}
        {error && (
          <p role="alert" className="text-sm text-destructive">
            {error}
          </p>
        )}
        <fieldset
          disabled={busy || !jsonValid}
          className="min-w-0 space-y-6 border-0 p-0 disabled:opacity-50"
        >
          <BasicFormFields
            form={form}
            beforeNameSlot={
              <div className="space-y-2">
                <Label htmlFor="codebuddy-model-id" className={REQUIRED_LABEL}>
                  {t("codebuddy.modelId")}
                </Label>
                {isEdit ? (
                  <Input
                    id="codebuddy-model-id"
                    value={modelId}
                    disabled
                    autoComplete="off"
                  />
                ) : (
                  <ModelInputWithFetch
                    id="codebuddy-model-id"
                    value={modelId}
                    onChange={setModelId}
                    placeholder="deepseek-chat"
                    fetchedModels={fetchedModels}
                    isLoading={isFetchingModels}
                    onFetch={handleFetchModels}
                  />
                )}
                <p
                  className={
                    keyTaken || keyInvalid
                      ? "text-xs text-destructive"
                      : "text-xs text-fg-2"
                  }
                >
                  {keyTaken
                    ? t("codebuddy.modelIdDuplicate")
                    : keyInvalid
                      ? t("codebuddy.modelIdInvalid")
                      : isEdit
                        ? t("codebuddy.modelIdLockedHint")
                        : t("codebuddy.modelIdHint")}
                </p>
              </div>
            }
          />
          <div className="space-y-2">
            <Label htmlFor="codebuddy-api">{t("codebuddy.api")}</Label>
            <Select
              value={config.api ? String(config.api) : CHAT_COMPLETIONS}
              onValueChange={(value) =>
                setField("api", value === CHAT_COMPLETIONS ? undefined : value)
              }
            >
              <SelectTrigger id="codebuddy-api">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {CODEBUDDY_API_FORMATS.map((format) => (
                  <SelectItem
                    key={format.value || CHAT_COMPLETIONS}
                    value={format.value || CHAT_COMPLETIONS}
                  >
                    {t(format.labelKey)}
                  </SelectItem>
                ))}
                {config.api &&
                  !CODEBUDDY_API_FORMATS.some(
                    (format) => format.value === config.api,
                  ) && (
                    <SelectItem value={String(config.api)}>
                      {String(config.api)}
                    </SelectItem>
                  )}
              </SelectContent>
            </Select>
          </div>
          <ApiKeySection
            id="codebuddy-api-key"
            value={apiKey}
            onChange={(value) => setField("apiKey", value)}
            category={category}
            shouldShowLink={Boolean(preset?.apiKeyUrl)}
            websiteUrl={preset?.apiKeyUrl ?? ""}
            isPartner={preset?.isPartner}
            partnerPromotionKey={preset?.partnerPromotionKey}
            placeholder={{
              official: t("codebuddy.apiKeyPlaceholder"),
              thirdParty: t("codebuddy.apiKeyPlaceholder"),
            }}
          />
          <div className="space-y-2">
            <EndpointField
              id="codebuddy-url"
              label={t("providerForm.apiEndpoint")}
              value={url}
              onChange={(value) => setField("url", value)}
              placeholder={
                config.api === "openai-responses"
                  ? "https://api.example.com/v1"
                  : "https://api.example.com/v1/chat/completions"
              }
              hint={
                urlInvalid
                  ? undefined
                  : config.api === "openai-responses"
                    ? t("codebuddy.urlHintResponses")
                    : t("codebuddy.urlHint")
              }
            />
            {urlInvalid && (
              <p className="text-xs text-destructive">
                {t("codebuddy.urlInvalid")}
              </p>
            )}
          </div>
          <div className="space-y-2">
            <Label htmlFor="codebuddy-vendor">{t("codebuddy.vendor")}</Label>
            <Input
              id="codebuddy-vendor"
              value={config.vendor ?? ""}
              onChange={(event) => setField("vendor", event.target.value)}
              placeholder="OpenAI"
              autoComplete="off"
            />
          </div>
          <div className="grid grid-cols-2 gap-3">
            {(["maxInputTokens", "maxOutputTokens"] as const).map((key) => (
              <div key={key} className="space-y-2">
                <Label htmlFor={`codebuddy-${key}`}>
                  {t(`codebuddy.${key}`)}
                </Label>
                <Input
                  id={`codebuddy-${key}`}
                  type="number"
                  min={1}
                  step={1}
                  value={config[key] === undefined ? "" : String(config[key])}
                  onChange={(event) =>
                    setField(key, toPositiveInt(event.target.value))
                  }
                  autoComplete="off"
                />
              </div>
            ))}
          </div>
          <div className="flex flex-wrap gap-4">
            {CAPABILITIES.map((key) => (
              <label key={key} className="flex items-center gap-2 text-sm">
                <Checkbox
                  checked={config[key] === true}
                  onCheckedChange={(checked) =>
                    setField(key, checked ? true : undefined)
                  }
                />
                {t(`codebuddy.${key}`)}
              </label>
            ))}
          </div>
        </fieldset>
        <div className="space-y-2">
          <Label htmlFor="codebuddy-settings-config">
            {t("provider.configJson")}
          </Label>
          <JsonEditor
            id="codebuddy-settings-config"
            ariaLabel={t("provider.configJson")}
            value={jsonText}
            darkMode={isDarkMode}
            language="json"
            showValidation
            height={Math.max(1, jsonText.split("\n").length) * 20 + 20}
            onChange={(text) => {
              setJsonText(text);
              try {
                const parsed: unknown = JSON.parse(text);
                if (!isModelConfig(parsed)) throw new Error("not an object");
                setConfig(withoutId(parsed));
                setJsonValid(true);
              } catch {
                setJsonValid(false);
              }
            }}
          />
          <p className="text-xs text-fg-2">{t("codebuddy.jsonHint")}</p>
        </div>
        {showButtons && (
          <div className="flex justify-end gap-2">
            <Button
              type="button"
              variant="outline"
              onClick={onCancel}
              disabled={busy}
            >
              {t("common.cancel")}
            </Button>
            <Button type="submit" disabled={busy || !ready}>
              {submitLabel}
            </Button>
          </div>
        )}
      </form>
    </Form>
  );
}
