import { useCallback, useEffect, useRef, useState } from "react";
import { useForm } from "react-hook-form";
import { useTranslation } from "react-i18next";
import { ChevronDown, ChevronRight, Plus, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Form } from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
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
import {
  fetchModelsForConfig,
  showFetchModelsError,
  type FetchedModel,
} from "@/lib/api/model-fetch";
import { toast } from "@/lib/toast";
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
import { BasicFormFields } from "./BasicFormFields";
import { ApiKeySection, ModelDropdown } from "./shared";
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
  const modelKeyPrefix = providerKey || "custom";
  const uniqueModelKey = (models: KimiProviderConfig["models"], id: string) => {
    let key = `${modelKeyPrefix}/${id}`;
    for (let index = 2; Object.prototype.hasOwnProperty.call(models, key); )
      key = `${modelKeyPrefix}/${id}-${index++}`;
    return key;
  };

  const [fetchedModels, setFetchedModels] = useState<FetchedModel[]>([]);
  const [isFetchingModels, setIsFetchingModels] = useState(false);
  const modelFetchGeneration = useRef(0);
  const { type: providerType, base_url: baseUrl = "" } = config.provider;
  const apiKey = config.provider.api_key ?? "";
  const headersKey = JSON.stringify(config.provider.custom_headers ?? {});
  // 请求头很少用到，收在高级选项里；已经有值（编辑已有卡、选了带请求头的预设）时展开。
  const hasCustomHeaders =
    Object.keys(config.provider.custom_headers ?? {}).length > 0;
  const [advancedExpanded, setAdvancedExpanded] = useState(hasCustomHeaders);
  useEffect(() => {
    if (hasCustomHeaders) setAdvancedExpanded(true);
  }, [hasCustomHeaders]);
  useEffect(() => {
    setFetchedModels((prev) => (prev.length === 0 ? prev : []));
    setIsFetchingModels(false);
    return () => {
      // 地址、Key、协议或请求头变了，旧请求的结果不再适用。
      modelFetchGeneration.current += 1;
    };
  }, [providerType, baseUrl, apiKey, headersKey]);

  const handleFetchModels = useCallback(() => {
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
  }, [apiKey, baseUrl, headersKey, providerType, t]);

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
        <p className="text-sm text-fg-2">{t("kimi.modelSelectionHint")}</p>
        <fieldset
          disabled={busy || !jsonValid}
          className="min-w-0 space-y-6 border-0 p-0 disabled:opacity-50"
        >
          <BasicFormFields
            form={form}
            beforeNameSlot={
              <div className="space-y-2">
                <Label htmlFor="kimi-provider-key">
                  {t("opencode.providerKey")} *
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
          <div className="space-y-2">
            <Label htmlFor="kimi-provider-type">{t("kimi.providerType")}</Label>
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
                    {type}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <div className="space-y-2">
            <Label htmlFor="kimi-base-url">{t("kimi.baseUrl")} *</Label>
            <Input
              id="kimi-base-url"
              value={config.provider.base_url ?? ""}
              onChange={(e) => updateProvider({ base_url: e.target.value })}
              placeholder="https://api.example.com/v1"
            />
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
          <Collapsible
            open={advancedExpanded}
            onOpenChange={setAdvancedExpanded}
            className="rounded-lg border border-border p-4"
          >
            <CollapsibleTrigger asChild>
              <Button
                type="button"
                variant={null}
                size="sm"
                className="h-8 w-full justify-start gap-1.5 px-0 text-sm font-medium text-fg-1 hover:opacity-70"
              >
                {advancedExpanded ? (
                  <ChevronDown className="h-4 w-4" />
                ) : (
                  <ChevronRight className="h-4 w-4" />
                )}
                {t("providerForm.advancedOptionsToggle")}
              </Button>
            </CollapsibleTrigger>
            <CollapsibleContent className="pt-2">
              <RequestHeadersEditor
                headers={config.provider.custom_headers ?? {}}
                onHeadersChange={(custom_headers) =>
                  updateProvider({ custom_headers })
                }
              />
            </CollapsibleContent>
          </Collapsible>
          <div className="space-y-3">
            <div className="flex items-center justify-between">
              <Label>{t("kimi.models")}</Label>
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
                  <Plus className="mr-1 h-4 w-4" />
                  {t("kimi.addModel")}
                </Button>
              </div>
            </div>
            {Object.entries(config.models).map(([key, model]) => (
              <div
                key={key}
                className="space-y-3 rounded-lg border border-border p-4"
              >
                <div className="flex items-center justify-between gap-2">
                  <span className="truncate text-sm text-fg-2">{key}</span>
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon"
                    aria-label={t("kimi.removeModel")}
                    onClick={() => {
                      const models = { ...config.models };
                      delete models[key];
                      placeholderContextKeys.current.delete(key);
                      update({ ...config, models });
                    }}
                  >
                    <Trash2 className="h-4 w-4" />
                  </Button>
                </div>
                <div className="grid grid-cols-2 gap-3">
                  <div className="space-y-2">
                    <Label htmlFor={`kimi-model-${key}`}>
                      {t("kimi.modelId")} *
                    </Label>
                    <div className="flex gap-1">
                      <Input
                        id={`kimi-model-${key}`}
                        value={model.model}
                        onChange={(e) =>
                          updateModel(key, { model: e.target.value })
                        }
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
                  </div>
                  <div className="space-y-2">
                    <Label htmlFor={`kimi-context-${key}`}>
                      {t("kimi.contextSize")} *
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
                    />
                  </div>
                </div>
                <div className="flex flex-wrap gap-4">
                  {KIMI_CAPABILITIES.map((capability) => (
                    <div key={capability} className="flex items-center gap-1">
                      <label className="flex items-center gap-2 text-sm">
                        <Checkbox
                          checked={
                            model.capabilities?.includes(capability) ?? false
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
                        {t(`kimi.capabilities.${capability}`)}
                      </label>
                      {CAPABILITIES_WITH_HELP.has(capability) && (
                        <HelpTip title={t(`kimi.capabilities.${capability}`)}>
                          {t(`kimi.capabilityHints.${capability}`)}
                        </HelpTip>
                      )}
                    </div>
                  ))}
                </div>
              </div>
            ))}
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
