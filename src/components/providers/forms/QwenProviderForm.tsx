import { useCallback, useEffect, useRef, useState } from "react";
import { useForm } from "react-hook-form";
import { useTranslation } from "react-i18next";
import { ChevronRight, Plus, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Form, FormLabel } from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import { ImeSafeInput } from "@/components/ui/ime-safe-input";
import { HelpTip } from "@/components/ui/help-tip";
import { HoverTip } from "@/components/ui/hover-tip";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import JsonEditor from "@/components/JsonEditor";
import { useDarkMode } from "@/hooks/useDarkMode";
import { useModelMetadataFill } from "@/hooks/useModelMetadataFill";
import {
  fetchModelsForConfig,
  showFetchModelsError,
  type FetchedModel,
} from "@/lib/api/model-fetch";
import { toast } from "@/lib/toast";
import { cn } from "@/lib/utils";
import { piPresetModelSources } from "@/config/presetModelMetadata";
import { useProvidersQuery } from "@/lib/query/queries";
import type { ProviderFormData } from "@/lib/schemas/provider";
import type { ProviderCategory } from "@/types";
import {
  emptyQwenConfig,
  qwenConfigSchema,
  qwenProviderPresets,
  QWEN_EDITABLE_AUTH_TYPES,
  type QwenAuthType,
  type QwenModel,
  type QwenProviderConfig,
} from "@/config/qwenProviderPresets";
import { BasicFormFields, REQUIRED_LABEL } from "./BasicFormFields";
import { ApiKeySection, EndpointField, GptContextWindowHint } from "./shared";
import {
  FetchedModelPicker,
  type FetchedModelSelection,
} from "./FetchedModelPicker";
import { ProviderPresetSelector } from "./ProviderPresetSelector";
import type { ProviderFormProps } from "./ProviderForm";

const PRESETS = qwenProviderPresets.map((preset, index) => ({
  id: String(index),
  preset,
}));

const PROVIDER_KEY_PATTERN = /^[a-z0-9]+(-[a-z0-9]+)*$/;

// The shared model-list command speaks the Pi protocol names.
const FETCH_API_FORMATS: Partial<Record<QwenAuthType, string>> = {
  openai: "openai-completions",
  "openai-responses": "openai-responses",
  anthropic: "anthropic-messages",
  gemini: "google-generative-ai",
};

/** Validation shared by the form fields and the JSON editor. */
export function qwenConfigProblems(config: QwenProviderConfig) {
  const ids = config.models.map((model) => model.id.trim());
  const baseUrl = config.baseUrl?.trim() ?? "";
  return {
    noModels: config.models.length === 0,
    emptyModelId: ids.some((id) => !id),
    duplicateModelIds: new Set(
      ids.filter((id, index) => id && ids.indexOf(id) !== index),
    ),
    invalidContext: config.models.some((model) => {
      const size = model.generationConfig?.contextWindowSize;
      return size !== undefined && !(Number.isInteger(size) && size > 0);
    }),
    // Anthropic has no implicit endpoint; models imported with their own
    // baseUrl already carry one.
    missingBaseUrl:
      config.authType === "anthropic" &&
      !baseUrl &&
      config.models.some((model) => !model.baseUrl?.trim()),
    invalidBaseUrl: Boolean(baseUrl) && !/^https?:\/\//.test(baseUrl),
  };
}

function normalizeConfig(config: QwenProviderConfig): QwenProviderConfig {
  const next: QwenProviderConfig = { ...config };
  if (!next.baseUrl?.trim()) delete next.baseUrl;
  else next.baseUrl = next.baseUrl.trim();
  if (!next.apiKey?.trim()) delete next.apiKey;
  next.models = config.models.map((model) => {
    const out: QwenModel = { ...model, id: model.id.trim() };
    if (!out.name?.trim()) delete out.name;
    return out;
  });
  return next;
}

export function QwenProviderForm({
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
  const darkMode = useDarkMode();
  // An unreadable stored profile stays in the JSON editor (blocking save)
  // instead of being silently replaced by an empty one.
  const [initial] = useState(() =>
    qwenConfigSchema.safeParse(
      initialData?.settingsConfig ?? emptyQwenConfig(),
    ),
  );
  const [config, setConfig] = useState<QwenProviderConfig>(() =>
    initial.success ? initial.data : emptyQwenConfig(),
  );
  const [text, setText] = useState(() =>
    JSON.stringify(
      initial.success ? initial.data : initialData?.settingsConfig,
      null,
      2,
    ),
  );
  const [jsonValid, setJsonValid] = useState(initial.success);
  const [providerKey, setProviderKey] = useState(providerId ?? "");
  const [presetId, setPresetId] = useState("custom");
  const [category, setCategory] = useState<ProviderCategory>(
    initialData?.category ?? "custom",
  );
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const { data: existing } = useProvidersQuery("qwen");
  const isEdit = Boolean(initialData);
  const duplicate =
    !isEdit &&
    Object.prototype.hasOwnProperty.call(
      existing?.providers ?? {},
      providerKey,
    );
  const keyValid = isEdit || PROVIDER_KEY_PATTERN.test(providerKey);
  const preset =
    presetId === "custom" ? undefined : qwenProviderPresets[Number(presetId)];
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
  const problems = qwenConfigProblems(config);
  const ready = Boolean(
    jsonValid &&
      name.trim() &&
      keyValid &&
      !duplicate &&
      !problems.noModels &&
      !problems.emptyModelId &&
      problems.duplicateModelIds.size === 0 &&
      !problems.invalidContext &&
      !problems.missingBaseUrl &&
      !problems.invalidBaseUrl,
  );
  useEffect(() => {
    onSubmitReadyChange?.(ready);
  }, [ready, onSubmitReadyChange]);

  // Metadata may arrive after later edits; fills apply to the latest config.
  const configRef = useRef(config);
  configRef.current = config;
  // Stable row keys keep the expanded state on the right model when rows move.
  const rowKeySeq = useRef(0);
  const rowKeys = useRef<string[]>([]);
  while (rowKeys.current.length < config.models.length) {
    rowKeys.current.push(`model-${rowKeySeq.current++}`);
  }
  rowKeys.current.length = config.models.length;
  const [expandedRows, setExpandedRows] = useState<Set<string>>(
    () => new Set(),
  );
  const toggleRow = (key: string) =>
    setExpandedRows((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  const update = (next: QwenProviderConfig) => {
    configRef.current = next;
    setConfig(next);
    setText(JSON.stringify(next, null, 2));
    setJsonValid(true);
  };

  const [fetchedModels, setFetchedModels] = useState<FetchedModel[]>([]);
  const [isFetchingModels, setIsFetchingModels] = useState(false);
  const fetchGeneration = useRef(0);
  const baseUrl = config.baseUrl?.trim() ?? "";
  const apiKey = config.apiKey?.trim() ?? "";
  const fillModelMetadata = useModelMetadataFill({
    baseUrl,
    presets: piPresetModelSources,
    prefetch: fetchedModels.length > 0,
  });
  const handleFetchModels = useCallback(() => {
    if (!baseUrl || !apiKey) {
      showFetchModelsError(null, t, {
        hasApiKey: Boolean(apiKey),
        hasBaseUrl: Boolean(baseUrl),
      });
      return;
    }
    const generation = ++fetchGeneration.current;
    setFetchedModels([]);
    setIsFetchingModels(true);
    fetchModelsForConfig(baseUrl, apiKey, undefined, undefined, undefined, {
      apiFormat: FETCH_API_FORMATS[config.authType],
    })
      .then((result) => {
        if (generation !== fetchGeneration.current) return;
        const models = [
          ...new Map(result.map((model) => [model.id, model])).values(),
        ];
        setFetchedModels(models);
        // The picker opens with the results; only an empty list needs a note.
        if (models.length === 0) toast.info(t("providerForm.fetchModelsEmpty"));
      })
      .catch((err) => {
        if (generation !== fetchGeneration.current) return;
        console.warn("[ModelFetch] Failed:", err);
        showFetchModelsError(err, t);
      })
      .finally(() => {
        if (generation === fetchGeneration.current) setIsFetchingModels(false);
      });
  }, [apiKey, baseUrl, config.authType, t]);
  const handleAddModel = () =>
    update({
      ...configRef.current,
      models: [...configRef.current.models, { id: "" }],
    });
  const handleRemoveModel = (index: number) => {
    const current = configRef.current;
    rowKeys.current.splice(index, 1);
    update({
      ...current,
      models: current.models.filter((_, i) => i !== index),
    });
  };
  const handleApplyFetchedModels = ({ add, remove }: FetchedModelSelection) => {
    const current = configRef.current;
    const keptIndexes = current.models
      .map((model, index) => ({ model, index }))
      .filter(({ model }) => !remove.includes(model.id.trim()));
    const present = new Set(keptIndexes.map(({ model }) => model.id.trim()));
    // Replace a single blank row instead of keeping it next to the picks.
    const base =
      keptIndexes.length === 1 &&
      !keptIndexes[0].model.id.trim() &&
      add.length > 0
        ? []
        : keptIndexes;
    const additions = add.filter((id) => !present.has(id));
    // Kept rows keep their keys (and expanded state); new rows get fresh ones.
    rowKeys.current = base.map(({ index }) => rowKeys.current[index]);
    update({
      ...current,
      models: [
        ...base.map(({ model }) => model),
        ...additions.map((id) => ({ id })),
      ],
    });
    for (const id of additions) {
      fillModelMetadata(
        id,
        (metadata) => {
          const latest = configRef.current;
          const index = latest.models.findIndex((model) => model.id === id);
          const window = metadata.contextWindow;
          const model = latest.models[index];
          if (
            index < 0 ||
            !window ||
            model.generationConfig?.contextWindowSize !== undefined
          )
            return false;
          update({
            ...latest,
            models: latest.models.map((entry, i) =>
              i === index
                ? {
                    ...entry,
                    generationConfig: {
                      ...entry.generationConfig,
                      contextWindowSize: window,
                    },
                  }
                : entry,
            ),
          });
          return true;
        },
        { silent: true },
      );
    }
  };
  const updateModel = (index: number, next: QwenModel) =>
    update({
      ...config,
      models: config.models.map((model, i) => (i === index ? next : model)),
    });
  const setContextWindow = (index: number, raw: string) => {
    const model = config.models[index];
    const generationConfig = { ...model.generationConfig };
    if (raw.trim() === "") delete generationConfig.contextWindowSize;
    else generationConfig.contextWindowSize = Number(raw);
    const next: QwenModel = { ...model, generationConfig };
    if (Object.keys(generationConfig).length === 0)
      delete next.generationConfig;
    updateModel(index, next);
  };

  const authTypes: QwenAuthType[] = (
    QWEN_EDITABLE_AUTH_TYPES as readonly QwenAuthType[]
  ).includes(config.authType)
    ? [...QWEN_EDITABLE_AUTH_TYPES]
    : [...QWEN_EDITABLE_AUTH_TYPES, config.authType];

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
            await onSubmit({
              ...identity,
              name: identity.name.trim(),
              providerKey: key,
              meta: initialData?.meta,
              presetCategory: category,
              settingsConfig: JSON.stringify(normalizeConfig(config)),
            });
          } catch (error) {
            setError(String(error));
          } finally {
            setBusy(false);
            onSubmittingChange?.(false);
          }
        })}
      >
        {!isEdit && (
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
              const next =
                id === "custom" ? undefined : qwenProviderPresets[Number(id)];
              setCategory(next?.category ?? "custom");
              update(
                qwenConfigSchema.parse(
                  structuredClone(next?.settingsConfig ?? emptyQwenConfig()),
                ),
              );
              setProviderKey(next?.providerKey ?? "");
              form.reset({
                name: next?.nameKey ? t(next.nameKey) : (next?.name ?? ""),
                notes: "",
                websiteUrl: next?.websiteUrl ?? "",
                icon: next?.icon ?? "",
                iconColor: next?.iconColor ?? "",
                settingsConfig: "",
              });
            }}
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
                <Label htmlFor="qwen-provider-key" className={REQUIRED_LABEL}>
                  {t("opencode.providerKey")}
                </Label>
                <Input
                  id="qwen-provider-key"
                  value={providerKey}
                  disabled={isEdit}
                  onChange={(e) =>
                    setProviderKey(
                      e.target.value.toLowerCase().replace(/[^a-z0-9-]/g, ""),
                    )
                  }
                  placeholder={t("opencode.providerKeyPlaceholder")}
                />
                <p
                  className={
                    duplicate || (providerKey && !keyValid)
                      ? "text-xs text-destructive"
                      : "text-xs text-fg-2"
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
            <FormLabel htmlFor="qwen-auth-type">
              {t("opencode.npmPackage")}
            </FormLabel>
            <Select
              value={config.authType}
              onValueChange={(authType) =>
                update({ ...config, authType: authType as QwenAuthType })
              }
            >
              <SelectTrigger id="qwen-auth-type">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {authTypes.map((type) => (
                  <SelectItem key={type} value={type}>
                    {t(`qwen.authTypes.${type}`)}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <ApiKeySection
            id="qwen-api-key"
            value={config.apiKey ?? ""}
            onChange={(apiKey) => update({ ...config, apiKey })}
            category={category}
            shouldShowLink={Boolean(preset?.apiKeyUrl)}
            websiteUrl={preset?.apiKeyUrl ?? ""}
            isPartner={preset?.isPartner}
            partnerPromotionKey={preset?.partnerPromotionKey}
          />
          <div className="space-y-2">
            <EndpointField
              id="qwen-base-url"
              label={t("opencode.baseUrl")}
              value={config.baseUrl ?? ""}
              onChange={(baseUrl) => update({ ...config, baseUrl })}
              placeholder="https://api.example.com/v1"
              required={config.authType === "anthropic"}
            />
            {problems.missingBaseUrl && (
              <p className="text-xs text-destructive">
                {t("qwen.baseUrlRequired")}
              </p>
            )}
            {problems.invalidBaseUrl && (
              <p className="text-xs text-destructive">
                {t("qwen.baseUrlInvalid")}
              </p>
            )}
          </div>
          {config.envKey && (
            <div className="space-y-2">
              <Label htmlFor="qwen-env-key">{t("qwen.envKey")}</Label>
              <Input id="qwen-env-key" value={config.envKey} readOnly />
              <p className="text-xs text-fg-2">{t("qwen.envKeyHint")}</p>
            </div>
          )}
          {/* Models editor: same layout as the OpenClaw / Hermes forms */}
          <div className="space-y-3 border-l border-border pl-3">
            <div className="flex items-center justify-between gap-3">
              <div className="flex items-center gap-1">
                <FormLabel>{t("opencode.models")}</FormLabel>
                <HelpTip title={t("opencode.models")}>
                  {t("qwen.modelSelectionHint")}
                </HelpTip>
              </div>
              <div className="flex gap-1">
                <FetchedModelPicker
                  models={fetchedModels}
                  configuredModelIds={config.models.map((model) =>
                    model.id.trim(),
                  )}
                  onApply={handleApplyFetchedModels}
                  onFetch={handleFetchModels}
                  isFetching={isFetchingModels}
                  metadataSource={{ baseUrl, presets: piPresetModelSources }}
                />
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  onClick={handleAddModel}
                  className="h-7 gap-1"
                >
                  <Plus className="h-3.5 w-3.5" />
                  {t("opencode.addModel")}
                </Button>
              </div>
            </div>

            {config.models.length === 0 ? (
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
                {config.models.map((model, index) => {
                  const rowKey = rowKeys.current[index];
                  const isExpanded = expandedRows.has(rowKey);
                  const duplicateId = problems.duplicateModelIds.has(
                    model.id.trim(),
                  );
                  const size = model.generationConfig?.contextWindowSize;
                  const override =
                    model.authType || model.baseUrl
                      ? [
                          model.authType &&
                            t(`qwen.authTypes.${model.authType}`),
                          model.baseUrl,
                        ]
                          .filter(Boolean)
                          .join(" · ")
                      : "";
                  return (
                    <div key={rowKey} className="space-y-2">
                      <div className="flex items-center gap-2">
                        <Button
                          type="button"
                          variant="ghost"
                          size="icon"
                          onClick={() => toggleRow(rowKey)}
                          aria-expanded={isExpanded}
                          aria-label={t("opencode.toggleModelDetails")}
                          className="h-9 w-9 shrink-0"
                        >
                          <ChevronRight
                            className={`h-4 w-4 transition-transform motion-reduce:transition-none ${
                              isExpanded ? "rotate-90" : ""
                            }`}
                          />
                        </Button>
                        <ImeSafeInput
                          value={model.id}
                          onValueChange={(value) =>
                            updateModel(index, { ...model, id: value })
                          }
                          placeholder="qwen3-coder-plus"
                          aria-label={t("opencode.modelId")}
                          aria-invalid={duplicateId || !model.id.trim()}
                          className="min-w-0 flex-1"
                        />
                        <ImeSafeInput
                          value={model.name ?? ""}
                          onValueChange={(value) =>
                            updateModel(index, { ...model, name: value })
                          }
                          placeholder={model.id.trim() || "Qwen3 Coder Plus"}
                          aria-label={t("opencode.modelName")}
                          className="min-w-0 flex-1"
                        />
                        <HoverTip content={t("qwen.removeModel")}>
                          <Button
                            type="button"
                            variant="ghost"
                            size="icon"
                            onClick={() => handleRemoveModel(index)}
                            aria-label={t("qwen.removeModel")}
                            className="h-9 w-9 shrink-0 text-fg-2 hover:text-destructive"
                          >
                            <Trash2 className="h-4 w-4" />
                          </Button>
                        </HoverTip>
                      </div>
                      {duplicateId && (
                        <p className="ml-11 text-xs text-destructive">
                          {t("qwen.duplicateModelId")}
                        </p>
                      )}

                      {isExpanded && (
                        <div className="ml-9 grid gap-3 border-l-2 border-muted pl-4 sm:grid-cols-2 animate-in fade-in-0 slide-in-from-top-1 duration-200 motion-reduce:animate-none">
                          <div className="space-y-1">
                            <Label
                              htmlFor={`qwen-context-${rowKey}`}
                              className="text-xs text-fg-2"
                            >
                              {t("qwen.contextWindowSize")}
                            </Label>
                            <Input
                              id={`qwen-context-${rowKey}`}
                              type="number"
                              min={1}
                              step={1}
                              value={size ?? ""}
                              onChange={(event) =>
                                setContextWindow(index, event.target.value)
                              }
                              placeholder="131072"
                            />
                            <GptContextWindowHint
                              modelId={model.id}
                              contextWindow={size}
                            />
                          </div>
                          {override && (
                            <div className="space-y-1 sm:col-span-2">
                              <span className="text-xs text-fg-2">
                                {t("qwen.modelOverride")}
                              </span>
                              <p className="break-all font-mono text-xs text-fg-1">
                                {override}
                              </p>
                            </div>
                          )}
                        </div>
                      )}
                    </div>
                  );
                })}
              </div>
            )}
          </div>
        </fieldset>
        <div className="space-y-2">
          <Label htmlFor="qwen-settings-config">
            {t("provider.configJson")}
          </Label>
          <JsonEditor
            id="qwen-settings-config"
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
                  return qwenConfigSchema.safeParse(JSON.parse(text));
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
        )}
      </form>
    </Form>
  );
}
