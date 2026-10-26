import { useCallback, useEffect, useRef, useState } from "react";
import { useForm } from "react-hook-form";
import { useTranslation } from "react-i18next";
import { ChevronRight, Plus, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Form, FormLabel } from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { HelpTip } from "@/components/ui/help-tip";
import { HoverTip } from "@/components/ui/hover-tip";
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
import {
  fetchModelsForConfig,
  showFetchModelsError,
  type FetchedModel,
} from "@/lib/api/model-fetch";
import { kimiOAuthApi } from "@/lib/api/kimiOAuth";
import { toast } from "@/lib/toast";
import { cn } from "@/lib/utils";
import { useProvidersQuery } from "@/lib/query/queries";
import type { ProviderFormData } from "@/lib/schemas/provider";
import type { ProviderCategory } from "@/types";
import {
  emptyKimiConfig,
  kimiConfigSchema,
  kimiProviderPresets,
  kimiAccountPreset,
  KIMI_ACCOUNT_PRESET_ID,
  KIMI_PROVIDER_TYPES,
  KIMI_CAPABILITIES,
  type KimiProviderConfig,
} from "@/config/kimiProviderPresets";
import { kimiPresetModelSources } from "@/config/presetModelMetadata";
import {
  findRequestHeaderValue,
  normalizeRequestHeaders,
} from "./helpers/requestHeaders";
import { BasicFormFields, REQUIRED_LABEL } from "./BasicFormFields";
import {
  ApiKeySection,
  EndpointField,
  GptContextWindowHint,
  ModelDropdown,
} from "./shared";
import {
  FetchedModelPicker,
  type FetchedModelSelection,
} from "./FetchedModelPicker";
import { fillKimiModel, metadataFilledAnything } from "./modelMetadataFill";
import { RequestHeadersEditor } from "./RequestHeadersEditor";
import { ProviderPresetSelector } from "./ProviderPresetSelector";
import type { ProviderFormProps } from "./ProviderForm";
import { usePresetStep } from "./presetStep";

// 新加的行先用这个窗口占位（max_context_size 必填），选中模型后换成查到的值。
const PLACEHOLDER_CONTEXT_SIZE = 262144;
// 名字看不出含义的两项，旁边放「?」说明。
const CAPABILITIES_WITH_HELP = new Set<string>([
  "always_thinking",
  "dynamically_loaded_tools",
]);

// 拉模型列表时按协议选认证头：Anthropic 用 x-api-key，Gemini 用 x-goog-api-key。
const MODEL_FETCH_API_FORMATS: Partial<
  Record<KimiProviderConfig["provider"]["type"], string>
> = {
  anthropic: "anthropic-messages",
  "google-genai": "google-generative-ai",
};

// 和 Pi、OpenCode 的「接口格式」下拉用同一套叫法。
const PROVIDER_TYPE_LABELS: Record<
  KimiProviderConfig["provider"]["type"],
  string
> = {
  kimi: "Kimi",
  openai: "OpenAI Chat Completions",
  openai_responses: "OpenAI Responses",
  anthropic: "Anthropic Messages",
  "google-genai": "Google Generative AI",
  vertexai: "Google Vertex AI",
};

const PRESETS = [
  { id: KIMI_ACCOUNT_PRESET_ID, preset: kimiAccountPreset },
  ...kimiProviderPresets.map((preset, index) => ({
    id: String(index),
    preset,
  })),
];

export function KimiProviderForm({
  providerId,
  initialData,
  onSubmit,
  onCancel,
  submitLabel,
  showButtons = true,
  onSubmittingChange,
  onSubmitReadyChange,
  onAccountLoginChange,
  onManageAuthAccounts,
}: ProviderFormProps) {
  const { t } = useTranslation();
  const darkMode = useDarkMode();
  const [config, setConfig] = useState<KimiProviderConfig>(() =>
    kimiConfigSchema.parse(initialData?.settingsConfig ?? emptyKimiConfig()),
  );
  const [text, setText] = useState(JSON.stringify(config, null, 2));
  const [jsonValid, setJsonValid] = useState(true);
  const [providerKey, setProviderKey] = useState(providerId ?? "");
  const [presetId, setPresetId] = useState("custom");
  const [category, setCategory] = useState<ProviderCategory>(
    initialData?.category ?? "custom",
  );
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const { data: existing } = useProvidersQuery("kimi");
  const isEdit = Boolean(initialData);
  const step = usePresetStep();
  const isAccountLogin = !isEdit && presetId === KIMI_ACCOUNT_PRESET_ID;
  const showAccountLogin = isAccountLogin && (!step || step.step === "form");
  useEffect(() => {
    onAccountLoginChange?.(showAccountLogin);
    return () => onAccountLoginChange?.(false);
  }, [showAccountLogin, onAccountLoginChange]);
  const duplicate =
    !isEdit &&
    Object.prototype.hasOwnProperty.call(
      existing?.providers ?? {},
      providerKey,
    );
  const keyValid = isEdit || /^[a-z0-9]+(-[a-z0-9]+)*$/.test(providerKey);
  const preset =
    presetId === "custom" ? undefined : kimiProviderPresets[Number(presetId)];
  const form = useForm<ProviderFormData>({
    defaultValues: {
      name: initialData?.name ?? "",
      notes: initialData?.notes ?? "",
      websiteUrl: initialData?.websiteUrl ?? "",
      icon: initialData?.icon ?? "",
      iconColor: initialData?.iconColor ?? "",
      settingsConfig: text,
    },
  });
  const name = form.watch("name");
  const modelsValid =
    Object.entries(config.models).length > 0 &&
    Object.entries(config.models).every(
      ([key, model]) =>
        key.trim() &&
        model.model.trim() &&
        Number.isInteger(model.max_context_size) &&
        model.max_context_size > 0,
    );
  const ready = Boolean(
    !isAccountLogin &&
      jsonValid &&
      name.trim() &&
      keyValid &&
      !duplicate &&
      ((isEdit && !config.provider.base_url) ||
        /^https?:\/\//.test(config.provider.base_url ?? "")) &&
      (config.provider.api_key?.trim() ||
        config.provider.oauth?.key ||
        config.provider.api_key_env ||
        Object.entries(config.provider.env ?? {}).some(
          ([key, value]) =>
            [
              "KIMI_API_KEY",
              "ANTHROPIC_API_KEY",
              "OPENAI_API_KEY",
              "GOOGLE_API_KEY",
              "VERTEXAI_API_KEY",
            ].includes(key) && value.trim(),
        )) &&
      modelsValid,
  );
  useEffect(() => {
    onSubmitReadyChange?.(ready);
  }, [ready, onSubmitReadyChange]);
  const configRef = useLatestRef(config);
  const update = (next: KimiProviderConfig) => {
    configRef.current = next;
    setConfig(next);
    setText(JSON.stringify(next, null, 2));
    setJsonValid(true);
  };
  // 窗口还是占位值的行（这次新加、用户没改过窗口），选中模型时可以换成查到的值。
  const placeholderContextKeys = useRef(new Set<string>());
  const [expandedModels, setExpandedModels] = useState<Set<string>>(
    () => new Set(),
  );
  const toggleModelExpand = (key: string) =>
    setExpandedModels((current) => {
      const next = new Set(current);
      if (!next.delete(key)) next.add(key);
      return next;
    });
  // 官方账号卡：连接和登录归授权中心，只编辑模型；模型键沿用登录时的凭据槽位前缀
  // （分开账号的供应商键是可读名，模型键得保持 ASCII 槽位名）。
  const officialSlot =
    isEdit && config.provider.type === "kimi"
      ? config.provider.oauth?.key?.replace(/^oauth\//, "")
      : undefined;
  const isOfficial = Boolean(officialSlot);
  const modelKeyPrefix = officialSlot || providerKey || "custom";
  const uniqueModelKey = (models: KimiProviderConfig["models"], id: string) => {
    let key = `${modelKeyPrefix}/${id}`;
    for (let index = 2; Object.prototype.hasOwnProperty.call(models, key); )
      key = `${modelKeyPrefix}/${id}-${index++}`;
    return key;
  };

  const [fetchedModels, setFetchedModels] = useState<FetchedModel[]>([]);
  const [isFetchingModels, setIsFetchingModels] = useState(false);
  // 官方账号拉到的完整目录：选中时直接用官方给的窗口、能力和思考档位
  const officialCatalog = useRef<KimiProviderConfig["models"]>({});
  const modelFetchGeneration = useRef(0);
  const { type: providerType, base_url: baseUrl = "" } = config.provider;
  const apiKey = config.provider.api_key ?? "";
  const headersKey = JSON.stringify(config.provider.custom_headers ?? {});
  useEffect(() => {
    setFetchedModels((prev) => (prev.length === 0 ? prev : []));
    setIsFetchingModels(false);
    return () => {
      // 地址、Key、协议或请求头变了，旧请求的结果不再适用。
      modelFetchGeneration.current += 1;
    };
  }, [providerType, baseUrl, apiKey, headersKey]);

  const handleFetchModels = useCallback(() => {
    if (isOfficial && providerId) {
      const generation = ++modelFetchGeneration.current;
      setFetchedModels([]);
      setIsFetchingModels(true);
      kimiOAuthApi
        .models(providerId)
        .then((catalog) => {
          if (generation !== modelFetchGeneration.current) return;
          officialCatalog.current = catalog;
          setFetchedModels(
            Object.values(catalog).map((model) => ({
              id: model.model,
              ownedBy: null,
            })),
          );
        })
        .catch((err) => {
          if (generation !== modelFetchGeneration.current) return;
          console.warn("[KimiOAuth] Failed to fetch models:", err);
          showFetchModelsError(err, t);
        })
        .finally(() => {
          if (generation === modelFetchGeneration.current) {
            setIsFetchingModels(false);
          }
        });
      return;
    }
    const endpoint = baseUrl.trim();
    const requestHeaders = normalizeRequestHeaders(
      JSON.parse(headersKey) as Record<string, string>,
    );
    const hasCredentials =
      Boolean(apiKey) || Object.keys(requestHeaders).length > 0;
    if (!endpoint || !hasCredentials) {
      showFetchModelsError(null, t, {
        hasApiKey: hasCredentials,
        hasBaseUrl: Boolean(endpoint),
      });
      return;
    }
    const generation = ++modelFetchGeneration.current;
    setFetchedModels([]);
    setIsFetchingModels(true);
    fetchModelsForConfig(
      endpoint,
      apiKey,
      undefined,
      undefined,
      findRequestHeaderValue(requestHeaders, "user-agent"),
      { apiFormat: MODEL_FETCH_API_FORMATS[providerType], requestHeaders },
    )
      .then((result) => {
        if (generation !== modelFetchGeneration.current) return;
        const models = [
          ...new Map(result.map((model) => [model.id, model])).values(),
        ];
        setFetchedModels(models);
        // 拉到模型时直接打开选择弹窗，不再提示成功。
        if (models.length === 0) toast.info(t("providerForm.fetchModelsEmpty"));
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
  }, [apiKey, baseUrl, headersKey, isOfficial, providerId, providerType, t]);

  const fillModelMetadata = useModelMetadataFill({
    baseUrl,
    presets: kimiPresetModelSources,
    prefetch: fetchedModels.length > 0,
  });
  // 补全可能晚到：读最新的配置，确认这一行仍是这个模型再补。
  const fillModelMetadataFor = (key: string, id: string, silent?: boolean) =>
    fillModelMetadata(
      id,
      (metadata) => {
        const current = configRef.current;
        const model = current.models[key];
        if (model?.model !== id) return false;
        const filled = fillKimiModel(model, metadata, {
          replaceContext: placeholderContextKeys.current.has(key),
        });
        if (!metadataFilledAnything(model, filled)) return false;
        placeholderContextKeys.current.delete(key);
        update({ ...current, models: { ...current.models, [key]: filled } });
        return true;
      },
      { silent },
    );

  const handleApplyFetchedModels = ({ add, remove }: FetchedModelSelection) => {
    const removed = new Set(remove);
    const models = Object.fromEntries(
      Object.entries(configRef.current.models).filter(
        ([, model]) => !removed.has(model.model),
      ),
    );
    const configured = new Set(Object.values(models).map((m) => m.model));
    const added: [string, string][] = [];
    for (const id of add) {
      if (configured.has(id)) continue;
      const official = Object.entries(officialCatalog.current).find(
        ([, model]) => model.model === id,
      );
      if (isOfficial && official) {
        const [catalogKey, model] = official;
        const key = Object.prototype.hasOwnProperty.call(models, catalogKey)
          ? uniqueModelKey(models, id)
          : catalogKey;
        models[key] = structuredClone(model);
        continue;
      }
      const key = uniqueModelKey(models, id);
      // Kimi 自己按目录导入模型时也默认支持工具调用。
      models[key] = {
        model: id,
        max_context_size: PLACEHOLDER_CONTEXT_SIZE,
        capabilities: ["tool_use"],
      };
      placeholderContextKeys.current.add(key);
      added.push([key, id]);
    }
    update({ ...configRef.current, models });
    // 弹窗里已经显示过这些参数，补完不再提示。
    for (const [key, id] of added) fillModelMetadataFor(key, id, true);
  };
  const updateProvider = (patch: Partial<KimiProviderConfig["provider"]>) =>
    update({ ...config, provider: { ...config.provider, ...patch } });
  const updateModel = (
    key: string,
    patch: Partial<KimiProviderConfig["models"][string]>,
  ) =>
    update({
      ...configRef.current,
      models: {
        ...configRef.current.models,
        [key]: { ...configRef.current.models[key], ...patch },
      },
    });
  const presetSelector = (
    <ProviderPresetSelector
      selectedPresetId={presetId}
      presetEntries={PRESETS}
      presetCategoryLabels={{
        custom: t("providerPreset.custom"),
        cn_official: t("providerForm.categoryCnOfficial"),
        third_party: t("providerPreset.group.thirdparty"),
        aggregator: t("providerPreset.group.thirdparty"),
        cloud_provider: t("providerPreset.group.cloud"),
        local: t("providerPreset.group.local"),
      }}
      category={category}
      onPresetChange={(id) => {
        setPresetId(id);
        if (id === KIMI_ACCOUNT_PRESET_ID) return;
        const preset =
          id === "custom" ? undefined : kimiProviderPresets[Number(id)];
        setCategory(preset?.category ?? "custom");
        placeholderContextKeys.current.clear();
        update(
          kimiConfigSchema.parse(
            structuredClone(preset?.settingsConfig ?? emptyKimiConfig()),
          ),
        );
        setProviderKey(preset?.providerKey ?? "");
        form.reset({
          name: preset?.nameKey ? t(preset.nameKey) : (preset?.name ?? ""),
          notes: "",
          websiteUrl: preset?.websiteUrl ?? "",
          icon: preset?.icon ?? "",
          iconColor: preset?.iconColor ?? "",
          settingsConfig: "",
        });
      }}
    />
  );
  if (isAccountLogin) {
    return (
      <Form {...form}>
        <form
          id="provider-form"
          className="space-y-6"
          onSubmit={(event) => event.preventDefault()}
        >
          {presetSelector}
          {showAccountLogin && (
            <div className="space-y-3">
              <p className="text-body text-fg-2">
                {t("kimi.oauth.description")}
              </p>
              <Button
                type="button"
                variant="neutral"
                onClick={() => onManageAuthAccounts?.("kimi_oauth")}
              >
                {t("kimi.oauth.manageAccounts")}
              </Button>
            </div>
          )}
        </form>
      </Form>
    );
  }
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
            const key = isEdit ? providerId! : providerKey;
            const models = Object.fromEntries(
              Object.entries(config.models).map(([id, model]) => [
                id,
                { ...model, provider: key },
              ]),
            );
            await onSubmit({
              ...identity,
              name: identity.name.trim(),
              providerKey: key,
              meta: initialData?.meta,
              presetCategory: category,
              settingsConfig: JSON.stringify({
                ...config,
                provider: {
                  ...config.provider,
                  ...(config.provider.custom_headers
                    ? {
                        custom_headers: normalizeRequestHeaders(
                          config.provider.custom_headers,
                        ),
                      }
                    : {}),
                },
                models,
              }),
            });
          } catch (error) {
            setError(String(error));
          } finally {
            setBusy(false);
            onSubmittingChange?.(false);
          }
        })}
      >
        {!isEdit && presetSelector}
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
                <Label htmlFor="kimi-provider-key" className={REQUIRED_LABEL}>
                  {t("opencode.providerKey")}
                </Label>
                <Input
                  id="kimi-provider-key"
                  value={providerKey}
                  disabled={isEdit}
                  onChange={(e) => {
                    const next = e.target.value
                      .toLowerCase()
                      .replace(/[^a-z0-9-]/g, "");
                    const rename = (key: string) =>
                      key.startsWith(`${modelKeyPrefix}/`)
                        ? `${next || "custom"}/${key.slice(modelKeyPrefix.length + 1)}`
                        : key;
                    const models = Object.fromEntries(
                      Object.entries(config.models).map(([key, model]) => [
                        rename(key),
                        model,
                      ]),
                    );
                    placeholderContextKeys.current = new Set(
                      [...placeholderContextKeys.current].map(rename),
                    );
                    setExpandedModels(
                      (current) => new Set([...current].map(rename)),
                    );
                    setProviderKey(next);
                    update({ ...config, models });
                  }}
                  placeholder={t("opencode.providerKeyPlaceholder")}
                />
                <p
                  className={
                    duplicate ? "text-xs text-destructive" : "text-xs text-fg-2"
                  }
                >
                  {duplicate
                    ? t("opencode.providerKeyDuplicate")
                    : isEdit
                      ? t("opencode.providerKeyLockedHint")
                      : t("opencode.providerKeyHint")}
                </p>
              </div>
            }
          />
          {isOfficial ? (
            <p className="text-sm text-fg-2">
              {t("kimi.oauth.connectionManaged")}
            </p>
          ) : (
            <>
              <div className="space-y-2">
                <FormLabel htmlFor="kimi-provider-type">
                  {t("opencode.npmPackage")}
                </FormLabel>
                <Select
                  value={config.provider.type}
                  onValueChange={(type) =>
                    updateProvider({
                      type: type as KimiProviderConfig["provider"]["type"],
                    })
                  }
                >
                  <SelectTrigger id="kimi-provider-type">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {KIMI_PROVIDER_TYPES.map((type) => (
                      <SelectItem key={type} value={type}>
                        {PROVIDER_TYPE_LABELS[type]}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
              <ApiKeySection
                id="kimi-api-key"
                value={config.provider.api_key ?? ""}
                onChange={(api_key) => updateProvider({ api_key })}
                category={category}
                shouldShowLink={Boolean(preset?.apiKeyUrl)}
                websiteUrl={preset?.apiKeyUrl ?? ""}
                isPartner={preset?.isPartner}
                partnerPromotionKey={preset?.partnerPromotionKey}
                required={!config.provider.oauth}
              />
              <EndpointField
                id="kimi-base-url"
                label={t("opencode.baseUrl")}
                value={config.provider.base_url ?? ""}
                onChange={(base_url) => updateProvider({ base_url })}
                placeholder="https://api.example.com/v1"
              />
              <RequestHeadersEditor
                headers={config.provider.custom_headers ?? {}}
                onHeadersChange={(custom_headers) =>
                  updateProvider({ custom_headers })
                }
              />
            </>
          )}
          <div className="space-y-3 border-l border-border pl-3">
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-1">
                <FormLabel>{t("opencode.models")}</FormLabel>
                <HelpTip title={t("opencode.models")}>
                  {t("kimi.modelSelectionHint")}
                </HelpTip>
              </div>
              <div className="flex gap-1">
                <FetchedModelPicker
                  models={fetchedModels}
                  configuredModelIds={Object.values(config.models).map(
                    (model) => model.model,
                  )}
                  onApply={handleApplyFetchedModels}
                  onFetch={handleFetchModels}
                  isFetching={isFetchingModels}
                  metadataSource={{ baseUrl, presets: kimiPresetModelSources }}
                />
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  className="h-7 gap-1"
                  onClick={() => {
                    let index = Object.keys(config.models).length + 1;
                    let key = `${modelKeyPrefix}/model-${index}`;
                    while (
                      Object.prototype.hasOwnProperty.call(config.models, key)
                    )
                      key = `${modelKeyPrefix}/model-${++index}`;
                    placeholderContextKeys.current.add(key);
                    update({
                      ...config,
                      models: {
                        ...config.models,
                        [key]: {
                          model: "",
                          max_context_size: PLACEHOLDER_CONTEXT_SIZE,
                        },
                      },
                    });
                  }}
                >
                  <Plus className="h-3.5 w-3.5" />
                  {t("opencode.addModel")}
                </Button>
              </div>
            </div>
            {Object.keys(config.models).length === 0 ? (
              <p className="py-2 text-sm text-fg-2">{t("opencode.noModels")}</p>
            ) : (
              <div className="space-y-2">
                <div className="mb-1 flex items-center gap-2 px-1 text-xs text-fg-2">
                  <span className="w-9" />
                  <span className={cn("flex-1", REQUIRED_LABEL)}>
                    {t("opencode.modelId")}
                  </span>
                  <span className="flex-1">{t("opencode.modelName")}</span>
                  <span className="w-9" />
                </div>
                {Object.entries(config.models).map(([key, model]) => (
                  <div key={key} className="space-y-2">
                    <div className="flex items-center gap-2">
                      <Button
                        type="button"
                        variant="ghost"
                        size="icon"
                        onClick={() => toggleModelExpand(key)}
                        aria-label={t("opencode.toggleModelDetails")}
                        aria-expanded={expandedModels.has(key)}
                        className="h-9 w-9 shrink-0"
                      >
                        <ChevronRight
                          className={cn(
                            "h-4 w-4 transition-transform motion-reduce:transition-none",
                            expandedModels.has(key) && "rotate-90",
                          )}
                        />
                      </Button>
                      <div className="flex min-w-0 flex-1 gap-1">
                        <Input
                          id={`kimi-model-${key}`}
                          value={model.model}
                          onChange={(e) =>
                            updateModel(key, { model: e.target.value })
                          }
                          placeholder="model-id"
                          aria-label={t("opencode.modelId")}
                          className="min-w-0 flex-1"
                        />
                        {fetchedModels.length > 0 && (
                          <ModelDropdown
                            models={fetchedModels}
                            onSelect={(id) => {
                              updateModel(key, { model: id });
                              fillModelMetadataFor(key, id);
                            }}
                          />
                        )}
                      </div>
                      <Input
                        id={`kimi-model-name-${key}`}
                        value={model.display_name ?? ""}
                        onChange={(e) =>
                          updateModel(key, {
                            display_name: e.target.value || undefined,
                          })
                        }
                        placeholder={t("opencode.modelName")}
                        aria-label={t("opencode.modelName")}
                        className="min-w-0 flex-1"
                      />
                      <HoverTip content={t("common.delete")}>
                        <Button
                          type="button"
                          variant="ghost"
                          size="icon"
                          aria-label={t("common.delete")}
                          className="h-9 w-9 shrink-0 text-fg-2 hover:text-destructive"
                          onClick={() => {
                            const models = { ...config.models };
                            delete models[key];
                            placeholderContextKeys.current.delete(key);
                            setExpandedModels((current) => {
                              const next = new Set(current);
                              next.delete(key);
                              return next;
                            });
                            update({ ...config, models });
                          }}
                        >
                          <Trash2 className="h-4 w-4" />
                        </Button>
                      </HoverTip>
                    </div>
                    {expandedModels.has(key) && (
                      <div className="ml-9 grid gap-3 border-l-2 border-muted pl-4 sm:grid-cols-2">
                        <div className="flex min-h-9 flex-wrap items-center gap-x-8 gap-y-2 sm:col-span-2">
                          {KIMI_CAPABILITIES.map((capability) => (
                            <div
                              key={capability}
                              className="flex items-center gap-2.5"
                            >
                              <div className="flex items-center gap-0.5">
                                <Label
                                  htmlFor={`kimi-model-${capability}-${key}`}
                                  className="cursor-pointer"
                                >
                                  {t(`kimi.capabilities.${capability}`)}
                                </Label>
                                {CAPABILITIES_WITH_HELP.has(capability) && (
                                  <HelpTip
                                    title={t(`kimi.capabilities.${capability}`)}
                                  >
                                    {t(`kimi.capabilityHints.${capability}`)}
                                  </HelpTip>
                                )}
                              </div>
                              <Switch
                                id={`kimi-model-${capability}-${key}`}
                                checked={
                                  model.capabilities?.includes(capability) ??
                                  false
                                }
                                onCheckedChange={(checked) =>
                                  updateModel(key, {
                                    capabilities: checked
                                      ? [
                                          ...new Set([
                                            ...(model.capabilities ?? []),
                                            capability,
                                          ]),
                                        ]
                                      : (model.capabilities ?? []).filter(
                                          (item) => item !== capability,
                                        ),
                                  })
                                }
                              />
                            </div>
                          ))}
                        </div>
                        <div className="space-y-2">
                          <Label
                            htmlFor={`kimi-context-${key}`}
                            className={REQUIRED_LABEL}
                          >
                            {t("kimi.contextSize")}
                          </Label>
                          <Input
                            id={`kimi-context-${key}`}
                            type="number"
                            min={1}
                            step={1}
                            value={model.max_context_size}
                            onChange={(e) => {
                              placeholderContextKeys.current.delete(key);
                              updateModel(key, {
                                max_context_size: Number(e.target.value),
                              });
                            }}
                            placeholder="262144"
                          />
                          <GptContextWindowHint
                            modelId={model.model}
                            contextWindow={model.max_context_size}
                          />
                        </div>
                      </div>
                    )}
                  </div>
                ))}
              </div>
            )}
          </div>
        </fieldset>
        <div className="space-y-2">
          <Label htmlFor="kimi-settings-config">
            {t("provider.configJson")}
          </Label>
          <JsonEditor
            id="kimi-settings-config"
            ariaLabel={t("provider.configJson")}
            value={text}
            darkMode={darkMode}
            language="json"
            showValidation
            height={Math.max(1, text.split("\n").length) * 20 + 20}
            onChange={(text) => {
              setText(text);
              const parsed = (() => {
                try {
                  return kimiConfigSchema.safeParse(JSON.parse(text));
                } catch {
                  return null;
                }
              })();
              setJsonValid(Boolean(parsed?.success));
              if (parsed?.success) setConfig(parsed.data);
            }}
          />
        </div>
        {showButtons && (
          <div className="flex justify-end gap-2">
            <Button
              type="button"
              variant="outline"
              disabled={busy}
              onClick={onCancel}
            >
              {t("common.cancel")}
            </Button>
            <Button type="submit" disabled={busy || !ready}>
              {submitLabel}
            </Button>
          </div>
        )}{" "}
      </form>
    </Form>
  );
}
