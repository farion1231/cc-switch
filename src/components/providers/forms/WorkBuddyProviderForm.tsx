import { useCallback, useEffect, useRef, useState } from "react";
import { z } from "zod";
import { useForm } from "react-hook-form";
import { useTranslation } from "react-i18next";
import { ChevronRight, Plus, Trash2 } from "lucide-react";
import { toast } from "@/lib/toast";
import { Button } from "@/components/ui/button";
import { Form, FormLabel } from "@/components/ui/form";
import { HelpTip } from "@/components/ui/help-tip";
import { HoverTip } from "@/components/ui/hover-tip";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import JsonEditor from "@/components/JsonEditor";
import { useDarkMode } from "@/hooks/useDarkMode";
import { useCommittableRef } from "@/hooks/useLatestRef";
import { useModelMetadataFill } from "@/hooks/useModelMetadataFill";
import {
  fetchModelsForConfig,
  showFetchModelsError,
  type FetchedModel,
} from "@/lib/api/model-fetch";
import { cn } from "@/lib/utils";
import { useProvidersQuery } from "@/lib/query/queries";
import type { ProviderFormData } from "@/lib/schemas/provider";
import { piPresetModelSources } from "@/config/presetModelMetadata";
import {
  WORKBUDDY_DEFAULT_CONFIG,
  workbuddyProviderPresets,
  type WorkBuddyModel,
  type WorkBuddySettingsConfig,
} from "@/config/workbuddyProviderPresets";
import { BasicFormFields, REQUIRED_LABEL } from "./BasicFormFields";
import {
  FetchedModelPicker,
  type FetchedModelSelection,
} from "./FetchedModelPicker";
import { ProviderPresetSelector } from "./ProviderPresetSelector";
import {
  fillWorkBuddyModel,
  metadataFilledAnything,
} from "./modelMetadataFill";
import { ApiKeySection, EndpointField, ModelDropdown } from "./shared";
import type { ProviderFormProps } from "./ProviderForm";

const PRESET_ENTRIES = workbuddyProviderPresets.map((preset, index) => ({
  id: String(index),
  preset,
}));

// WorkBuddy 的自定义模型这里只配 OpenAI Chat Completions。旧条目里的 `api` 字段
// 不在表单里展示，靠 passthrough 原样带回保存。
const CHAT_COMPLETIONS = "openai-completions";

const configSchema = z
  .object({
    vendor: z.string().optional(),
    url: z.string(),
    apiKey: z.string(),
    models: z.array(
      z
        .object({
          id: z.string(),
          name: z.string().optional(),
          maxInputTokens: z.number().optional(),
          maxOutputTokens: z.number().optional(),
          supportsToolCall: z.boolean().optional(),
          supportsImages: z.boolean().optional(),
          supportsReasoning: z.boolean().optional(),
        })
        .passthrough(),
    ),
  })
  .passthrough();

const KEY_PATTERN = /^[a-z0-9]+(-[a-z0-9]+)*$/;
const normalizeKey = (value: string) =>
  value.toLowerCase().replace(/[^a-z0-9-]/g, "");

// 能力项和 Kimi 的同名开关共用文案。
const CAPABILITIES = [
  ["supportsToolCall", "kimi.capabilities.tool_use"],
  ["supportsImages", "kimi.capabilities.image_in"],
  ["supportsReasoning", "kimi.capabilities.thinking"],
] as const;

const parseTokens = (value: string): number | undefined => {
  const parsed = Number.parseInt(value, 10);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : undefined;
};

/** Chat Completions 填的是完整地址；拉模型列表和查预设参数都按基础地址来。 */
const baseUrlOf = (config: WorkBuddySettingsConfig) =>
  config.url.trim().replace(/\/chat\/completions\/*$/, "");

export function WorkBuddyProviderForm({
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
  const [config, setConfig] = useState<WorkBuddySettingsConfig>(
    (initialData?.settingsConfig as WorkBuddySettingsConfig | undefined) ??
      WORKBUDDY_DEFAULT_CONFIG,
  );
  const [jsonText, setJsonText] = useState(JSON.stringify(config, null, 2));
  const [jsonValid, setJsonValid] = useState(true);
  const [presetId, setPresetId] = useState("custom");
  const preset =
    presetId === "custom"
      ? undefined
      : workbuddyProviderPresets[Number(presetId)];
  const category = initialData?.category ?? preset?.category ?? "custom";
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  // The key is stored on every models.json entry this provider owns, so it is
  // fixed once the provider exists.
  const isEdit = Boolean(initialData);
  const [providerKey, setProviderKey] = useState(providerId ?? "");
  const { data: existing } = useProvidersQuery("workbuddy");
  const keyTaken =
    !isEdit &&
    Object.prototype.hasOwnProperty.call(
      existing?.providers ?? {},
      providerKey,
    );
  const keyInvalid =
    !isEdit && providerKey !== "" && !KEY_PATTERN.test(providerKey);

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
  const models = config.models ?? [];
  const modelIds = models.map((model) => model.id.trim());
  const modelsValid =
    models.length > 0 &&
    modelIds.every(Boolean) &&
    new Set(modelIds).size === modelIds.length;
  const urlValid = /^https?:\/\/\S+$/.test(config.url?.trim() ?? "");
  const ready = Boolean(
    jsonValid &&
      name.trim() &&
      (isEdit || (providerKey && !keyInvalid && !keyTaken)) &&
      urlValid &&
      modelsValid,
  );
  useEffect(() => {
    onSubmitReadyChange?.(ready);
  }, [ready, onSubmitReadyChange]);

  const update = (next: WorkBuddySettingsConfig) => {
    setConfig(next);
    setJsonText(JSON.stringify(next, null, 2));
    setJsonValid(true);
  };
  // 补全参数可能晚到，异步回调经 ref 读最新配置再提交。
  const [configRef, commitConfig] = useCommittableRef(config, update);
  const commitModels = (next: WorkBuddyModel[]) =>
    commitConfig({ ...configRef.current, models: next });
  const updateModel = (index: number, patch: Partial<WorkBuddyModel>) =>
    commitModels(
      configRef.current.models.map((model, i) =>
        i === index ? { ...model, ...patch } : model,
      ),
    );
  // 模型是数组，展开状态按下标记；删除一行时把后面的下标往前挪。
  const [expandedModels, setExpandedModels] = useState<Set<number>>(
    () => new Set(),
  );
  const toggleModelExpand = (index: number) =>
    setExpandedModels((current) => {
      const next = new Set(current);
      if (!next.delete(index)) next.add(index);
      return next;
    });
  const removeModel = (index: number) => {
    commitModels(configRef.current.models.filter((_, i) => i !== index));
    setExpandedModels(
      (current) =>
        new Set(
          [...current]
            .filter((i) => i !== index)
            .map((i) => (i > index ? i - 1 : i)),
        ),
    );
  };

  // ── 从接口拉模型列表 ──
  const baseUrl = baseUrlOf(config);
  const apiKey = config.apiKey?.trim() ?? "";
  const [fetchedModels, setFetchedModels] = useState<FetchedModel[]>([]);
  const [isFetchingModels, setIsFetchingModels] = useState(false);
  const modelFetchGeneration = useRef(0);
  useEffect(() => {
    setFetchedModels((prev) => (prev.length === 0 ? prev : []));
    setIsFetchingModels(false);
    return () => {
      // Ignore responses for a previous endpoint/key or an unmounted form.
      modelFetchGeneration.current += 1;
    };
  }, [baseUrl, apiKey]);

  const handleFetchModels = useCallback(() => {
    if (!baseUrl || !apiKey) {
      showFetchModelsError(null, t, {
        hasApiKey: Boolean(apiKey),
        hasBaseUrl: Boolean(baseUrl),
      });
      return;
    }
    const generation = ++modelFetchGeneration.current;
    setFetchedModels([]);
    setIsFetchingModels(true);
    fetchModelsForConfig(baseUrl, apiKey, undefined, undefined, undefined, {
      apiFormat: CHAT_COMPLETIONS,
    })
      .then((result) => {
        if (generation !== modelFetchGeneration.current) return;
        const unique = [
          ...new Map(result.map((model) => [model.id, model])).values(),
        ];
        setFetchedModels(unique);
        // 拉到模型时直接打开选择弹窗，不再提示成功。
        if (unique.length === 0) {
          toast.info(t("providerForm.fetchModelsEmpty"));
        }
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
  }, [apiKey, baseUrl, t]);

  const fillModelMetadata = useModelMetadataFill({
    baseUrl,
    presets: piPresetModelSources,
    prefetch: fetchedModels.length > 0,
  });
  const fillModelMetadataFor = (id: string, silent?: boolean) =>
    fillModelMetadata(
      id,
      (metadata) => {
        const current = configRef.current.models;
        const index = current.findIndex((model) => model.id === id);
        if (index < 0) return false;
        const filled = fillWorkBuddyModel(current[index], metadata);
        if (!metadataFilledAnything(current[index], filled)) return false;
        commitModels(current.map((model, i) => (i === index ? filled : model)));
        return true;
      },
      { silent },
    );

  const applyFetchedModels = ({ add, remove }: FetchedModelSelection) => {
    const kept = configRef.current.models.filter(
      (model) => !remove.includes(model.id.trim()),
    );
    const keptIds = new Set(kept.map((model) => model.id.trim()));
    const additions = add
      .filter((id) => !keptIds.has(id))
      .map((id) => ({ id, supportsToolCall: true }));
    // 删了行，下标对不上了，收起全部。
    if (kept.length !== configRef.current.models.length)
      setExpandedModels(new Set());
    commitModels([...kept, ...additions]);
    // 弹窗里已经显示过这些参数，补完不再提示。
    for (const { id } of additions) fillModelMetadataFor(id, true);
  };

  const choosePreset = (id: string) => {
    setPresetId(id);
    const selected =
      id === "custom" ? undefined : workbuddyProviderPresets[Number(id)];
    const next = selected?.settingsConfig ?? WORKBUDDY_DEFAULT_CONFIG;
    update(next);
    setProviderKey(selected?.providerKey ?? "");
    form.reset({
      name: selected?.name ?? "",
      notes: "",
      websiteUrl: selected?.websiteUrl ?? "",
      icon: selected?.icon ?? "",
      iconColor: selected?.iconColor ?? "",
      settingsConfig: JSON.stringify(next),
    });
  };

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
            const cleaned: WorkBuddySettingsConfig = {
              ...config,
              url: config.url.trim(),
              apiKey: config.apiKey.trim(),
              models: models.map((model) => ({
                ...model,
                id: model.id.trim(),
              })),
            };
            if (!cleaned.vendor?.trim()) delete cleaned.vendor;
            await onSubmit({
              ...identity,
              name: identity.name.trim(),
              meta: initialData?.meta,
              providerKey: isEdit ? providerId : providerKey,
              presetCategory: category,
              settingsConfig: JSON.stringify(cleaned),
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
                <Label
                  htmlFor="workbuddy-provider-key"
                  className={REQUIRED_LABEL}
                >
                  {t("opencode.providerKey")}
                </Label>
                <Input
                  id="workbuddy-provider-key"
                  value={providerKey}
                  onChange={(event) =>
                    setProviderKey(normalizeKey(event.target.value))
                  }
                  disabled={isEdit}
                  placeholder={t("opencode.providerKeyPlaceholder")}
                  autoComplete="off"
                  aria-required
                  aria-invalid={keyTaken || keyInvalid || undefined}
                  className={keyTaken || keyInvalid ? "border-destructive" : ""}
                />
                <p
                  className={
                    keyTaken || keyInvalid
                      ? "text-xs text-destructive"
                      : "text-xs text-fg-2"
                  }
                >
                  {keyTaken
                    ? t("opencode.providerKeyDuplicate")
                    : keyInvalid
                      ? t("opencode.providerKeyInvalid")
                      : isEdit
                        ? t("workbuddy.form.providerKeyLockedHint")
                        : t("workbuddy.form.providerKeyHint")}
                </p>
              </div>
            }
          />

          <ApiKeySection
            id="workbuddy-api-key"
            value={config.apiKey}
            onChange={(apiKey) => update({ ...config, apiKey })}
            category={category}
            shouldShowLink={Boolean(preset?.apiKeyUrl)}
            websiteUrl={preset?.apiKeyUrl ?? ""}
            isPartner={preset?.isPartner}
            partnerPromotionKey={preset?.partnerPromotionKey}
            hint={t("workbuddy.form.apiKeyHint")}
          />

          <EndpointField
            id="workbuddy-url"
            label={t("workbuddy.form.url")}
            value={config.url}
            onChange={(url) => update({ ...config, url })}
            placeholder="https://api.example.com/v1/chat/completions"
            hint={t("workbuddy.form.urlHint")}
            showManageButton={false}
          />

          <div className="space-y-2">
            <Label htmlFor="workbuddy-vendor">
              {t("workbuddy.form.vendor")}
            </Label>
            <Input
              id="workbuddy-vendor"
              value={config.vendor ?? ""}
              onChange={(event) =>
                update({ ...config, vendor: event.target.value })
              }
              placeholder="Custom"
              autoComplete="off"
            />
          </div>

          <div className="space-y-3 border-l border-border pl-3">
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-1">
                <FormLabel>{t("opencode.models")}</FormLabel>
                <HelpTip title={t("opencode.models")}>
                  {t("workbuddy.form.modelsHint")}
                </HelpTip>
              </div>
              <div className="flex gap-1">
                <FetchedModelPicker
                  models={fetchedModels}
                  configuredModelIds={modelIds}
                  onApply={applyFetchedModels}
                  onFetch={handleFetchModels}
                  isFetching={isFetchingModels}
                  metadataSource={{ baseUrl, presets: piPresetModelSources }}
                />
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  className="h-7 gap-1"
                  onClick={() =>
                    commitModels([
                      ...configRef.current.models,
                      { id: "", supportsToolCall: true, supportsImages: false },
                    ])
                  }
                >
                  <Plus className="h-3.5 w-3.5" />
                  {t("opencode.addModel")}
                </Button>
              </div>
            </div>
            {models.length === 0 ? (
              <p role="status" className="py-2 text-sm text-fg-2">
                {t("opencode.noModels")}
              </p>
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
                {models.map((model, index) => (
                  <div key={index} className="space-y-2">
                    <div className="flex items-center gap-2">
                      <Button
                        type="button"
                        variant="ghost"
                        size="icon"
                        onClick={() => toggleModelExpand(index)}
                        aria-label={t("opencode.toggleModelDetails")}
                        aria-expanded={expandedModels.has(index)}
                        className="h-9 w-9 shrink-0"
                      >
                        <ChevronRight
                          className={cn(
                            "h-4 w-4 transition-transform motion-reduce:transition-none",
                            expandedModels.has(index) && "rotate-90",
                          )}
                        />
                      </Button>
                      <div className="flex min-w-0 flex-1 gap-1">
                        <Input
                          id={`workbuddy-model-id-${index}`}
                          value={model.id}
                          onChange={(event) =>
                            updateModel(index, { id: event.target.value })
                          }
                          placeholder="model-id"
                          aria-label={t("opencode.modelId")}
                          aria-required
                          autoComplete="off"
                          className="min-w-0 flex-1"
                        />
                        {fetchedModels.length > 0 && (
                          <ModelDropdown
                            models={fetchedModels}
                            onSelect={(id) => {
                              updateModel(index, { id });
                              fillModelMetadataFor(id);
                            }}
                          />
                        )}
                      </div>
                      <Input
                        id={`workbuddy-model-name-${index}`}
                        value={model.name ?? ""}
                        onChange={(event) =>
                          updateModel(index, {
                            name: event.target.value || undefined,
                          })
                        }
                        placeholder={t("opencode.modelName")}
                        aria-label={t("opencode.modelName")}
                        autoComplete="off"
                        className="min-w-0 flex-1"
                      />
                      <HoverTip content={t("common.delete")}>
                        <Button
                          type="button"
                          variant="ghost"
                          size="icon"
                          aria-label={t("common.delete")}
                          className="h-9 w-9 shrink-0 text-fg-2 hover:text-destructive"
                          onClick={() => removeModel(index)}
                        >
                          <Trash2 className="h-4 w-4" />
                        </Button>
                      </HoverTip>
                    </div>
                    {expandedModels.has(index) && (
                      <div className="ml-9 grid gap-3 border-l-2 border-muted pl-4 sm:grid-cols-2">
                        <div className="flex min-h-9 flex-wrap items-center gap-x-8 gap-y-2 sm:col-span-2">
                          {CAPABILITIES.map(([capability, label]) => (
                            <div
                              key={capability}
                              className="flex items-center gap-2.5"
                            >
                              <Label
                                htmlFor={`workbuddy-model-${capability}-${index}`}
                                className="cursor-pointer"
                              >
                                {t(label)}
                              </Label>
                              <Switch
                                id={`workbuddy-model-${capability}-${index}`}
                                checked={model[capability] === true}
                                onCheckedChange={(checked) =>
                                  updateModel(index, { [capability]: checked })
                                }
                              />
                            </div>
                          ))}
                        </div>
                        <div className="space-y-2">
                          <Label htmlFor={`workbuddy-model-input-${index}`}>
                            {t("workbuddy.form.maxInputTokens")}
                          </Label>
                          <Input
                            id={`workbuddy-model-input-${index}`}
                            type="number"
                            min={1}
                            step={1}
                            value={model.maxInputTokens ?? ""}
                            onChange={(event) =>
                              updateModel(index, {
                                maxInputTokens: parseTokens(event.target.value),
                              })
                            }
                            placeholder="128000"
                          />
                        </div>
                        <div className="space-y-2">
                          <Label htmlFor={`workbuddy-model-output-${index}`}>
                            {t("workbuddy.form.maxOutputTokens")}
                          </Label>
                          <Input
                            id={`workbuddy-model-output-${index}`}
                            type="number"
                            min={1}
                            step={1}
                            value={model.maxOutputTokens ?? ""}
                            onChange={(event) =>
                              updateModel(index, {
                                maxOutputTokens: parseTokens(
                                  event.target.value,
                                ),
                              })
                            }
                            placeholder="16384"
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
          <Label htmlFor="workbuddy-settings-config">
            {t("provider.configJson")}
          </Label>
          <JsonEditor
            id="workbuddy-settings-config"
            ariaLabel={t("provider.configJson")}
            value={jsonText}
            darkMode={isDarkMode}
            language="json"
            showValidation
            height={Math.max(1, jsonText.split("\n").length) * 20 + 20}
            onChange={(text) => {
              setJsonText(text);
              try {
                setConfig(configSchema.parse(JSON.parse(text)));
                setJsonValid(true);
              } catch {
                setJsonValid(false);
              }
            }}
          />
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
