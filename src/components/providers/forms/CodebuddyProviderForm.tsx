import { useCallback, useEffect, useRef, useState } from "react";
import { useForm } from "react-hook-form";
import { useTranslation } from "react-i18next";
import { ChevronRight, Download, Loader2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Form, FormLabel } from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { HelpTip } from "@/components/ui/help-tip";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import JsonEditor from "@/components/JsonEditor";
import { useDarkMode } from "@/hooks/useDarkMode";
import { useLatestRef } from "@/hooks/useLatestRef";
import { useModelMetadataFill } from "@/hooks/useModelMetadataFill";
import { toast } from "@/lib/toast";
import { cn } from "@/lib/utils";
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
import { codebuddyPresetModelSources } from "@/config/presetModelMetadata";
import { BasicFormFields, REQUIRED_LABEL } from "./BasicFormFields";
import { ApiKeySection, EndpointField, ModelDropdown } from "./shared";
import {
  fillCodebuddyModel,
  metadataFilledAnything,
} from "./modelMetadataFill";
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
  ["supportsToolCall", "kimi.capabilities.tool_use"],
  ["supportsImages", "kimi.capabilities.image_in"],
  ["supportsReasoning", "kimi.capabilities.thinking"],
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

  const configRef = useLatestRef(config);
  const modelIdRef = useLatestRef(trimmedId);
  const update = (next: CodebuddyModelConfig) => {
    configRef.current = next;
    setConfig(next);
    setJsonText(JSON.stringify(next, null, 2));
    setJsonValid(true);
  };
  const setField = (key: keyof CodebuddyModelConfig, value: unknown) => {
    const next = { ...configRef.current };
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
  const api = config.api;
  useEffect(() => {
    setFetchedModels((prev) => (prev.length === 0 ? prev : []));
    setIsFetchingModels(false);
    return () => {
      // 地址、Key 或接口格式变了，旧请求的结果不再适用。
      fetchGeneration.current += 1;
    };
  }, [url, apiKey, api]);
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

  const fillModelMetadata = useModelMetadataFill({
    baseUrl: url,
    presets: codebuddyPresetModelSources,
    prefetch: fetchedModels.length > 0,
  });
  const selectFetchedModel = (id: string) => {
    setModelId(id);
    modelIdRef.current = id;
    // 补全可能晚到：确认模型 ID 仍是这个再补。
    fillModelMetadata(id, (metadata) => {
      if (modelIdRef.current !== id) return false;
      const current = configRef.current;
      const filled = fillCodebuddyModel(current, metadata);
      if (!metadataFilledAnything(current, filled)) return false;
      update(filled);
      return true;
    });
  };
  const [expanded, setExpanded] = useState(false);

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
          <BasicFormFields form={form} />
          <div className="space-y-2">
            <FormLabel htmlFor="codebuddy-api">
              {t("opencode.npmPackage")}
            </FormLabel>
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
                    {format.label}
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
          {/* 一张卡片就是 models.json 里的一条模型，所以只有固定的一行。 */}
          <div className="space-y-3 border-l border-border pl-3">
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-1">
                <FormLabel>{t("opencode.models")}</FormLabel>
                <HelpTip title={t("opencode.models")}>
                  {t("codebuddy.modelsHint")}
                </HelpTip>
              </div>
              {!isEdit && (
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
              )}
            </div>
            <div className="space-y-2">
              <div className="mb-1 flex items-center gap-2 px-1 text-xs text-fg-2">
                <span className="w-9" />
                <span className={cn("flex-1", REQUIRED_LABEL)}>
                  {t("opencode.modelId")}
                </span>
                <span className={cn("flex-1", REQUIRED_LABEL)}>
                  {t("opencode.modelName")}
                </span>
              </div>
              <div className="space-y-2">
                <div className="flex items-center gap-2">
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon"
                    onClick={() => setExpanded((open) => !open)}
                    aria-label={t("opencode.toggleModelDetails")}
                    aria-expanded={expanded}
                    className="h-9 w-9 shrink-0"
                  >
                    <ChevronRight
                      className={cn(
                        "h-4 w-4 transition-transform motion-reduce:transition-none",
                        expanded && "rotate-90",
                      )}
                    />
                  </Button>
                  <div className="flex min-w-0 flex-1 gap-1">
                    <Input
                      id="codebuddy-model-id"
                      value={modelId}
                      onChange={(event) => setModelId(event.target.value)}
                      disabled={isEdit}
                      placeholder="deepseek-chat"
                      aria-label={t("opencode.modelId")}
                      aria-invalid={keyTaken || keyInvalid}
                      autoComplete="off"
                      className="min-w-0 flex-1"
                    />
                    {!isEdit && fetchedModels.length > 0 && (
                      <ModelDropdown
                        models={fetchedModels}
                        onSelect={selectFetchedModel}
                      />
                    )}
                  </div>
                  {/* models.json 的 name 同时是卡片名称，和上方「供应商名称」是同一个值。 */}
                  <Input
                    id="codebuddy-model-name"
                    value={name}
                    onChange={(event) =>
                      form.setValue("name", event.target.value, {
                        shouldDirty: true,
                      })
                    }
                    placeholder={t("opencode.modelName")}
                    aria-label={t("opencode.modelName")}
                    autoComplete="off"
                    className="min-w-0 flex-1"
                  />
                </div>
                {(keyTaken || keyInvalid) && (
                  <p className="ml-11 text-xs text-destructive">
                    {keyTaken
                      ? t("codebuddy.modelIdDuplicate")
                      : t("codebuddy.modelIdInvalid")}
                  </p>
                )}
                {expanded && (
                  <div className="ml-9 grid gap-3 border-l-2 border-muted pl-4 sm:grid-cols-2">
                    <div className="flex min-h-9 flex-wrap items-center gap-x-8 gap-y-2 sm:col-span-2">
                      {CAPABILITIES.map(([key, label]) => (
                        <div key={key} className="flex items-center gap-2.5">
                          <Label
                            htmlFor={`codebuddy-${key}`}
                            className="cursor-pointer"
                          >
                            {t(label)}
                          </Label>
                          <Switch
                            id={`codebuddy-${key}`}
                            checked={config[key] === true}
                            onCheckedChange={(checked) =>
                              setField(key, checked ? true : undefined)
                            }
                          />
                        </div>
                      ))}
                    </div>
                    {(["maxInputTokens", "maxOutputTokens"] as const).map(
                      (key) => (
                        <div key={key} className="space-y-2">
                          <Label htmlFor={`codebuddy-${key}`}>
                            {t(`codebuddy.${key}`)}
                          </Label>
                          <Input
                            id={`codebuddy-${key}`}
                            type="number"
                            min={1}
                            step={1}
                            value={
                              config[key] === undefined
                                ? ""
                                : String(config[key])
                            }
                            onChange={(event) =>
                              setField(key, toPositiveInt(event.target.value))
                            }
                            autoComplete="off"
                          />
                        </div>
                      ),
                    )}
                  </div>
                )}
              </div>
            </div>
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
