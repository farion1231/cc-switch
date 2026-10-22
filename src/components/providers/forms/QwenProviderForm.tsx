import { useEffect, useState } from "react";
import { useForm } from "react-hook-form";
import { useTranslation } from "react-i18next";
import { Plus, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Form } from "@/components/ui/form";
import { Input } from "@/components/ui/input";
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
import { BasicFormFields } from "./BasicFormFields";
import { ApiKeySection } from "./shared";
import { ProviderPresetSelector } from "./ProviderPresetSelector";
import type { ProviderFormProps } from "./ProviderForm";

const PRESETS = qwenProviderPresets.map((preset, index) => ({
  id: String(index),
  preset,
}));

const PROVIDER_KEY_PATTERN = /^[a-z0-9]+(-[a-z0-9]+)*$/;

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

  const update = (next: QwenProviderConfig) => {
    setConfig(next);
    setText(JSON.stringify(next, null, 2));
    setJsonValid(true);
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
        <p className="text-sm text-fg-2">{t("qwen.modelSelectionHint")}</p>
        <fieldset
          disabled={busy || !jsonValid}
          className="min-w-0 space-y-6 border-0 p-0 disabled:opacity-50"
        >
          <BasicFormFields
            form={form}
            beforeNameSlot={
              <div className="space-y-2">
                <Label htmlFor="qwen-provider-key">
                  {t("opencode.providerKey")} *
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
            <Label htmlFor="qwen-auth-type">{t("qwen.authType")}</Label>
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
          <div className="space-y-2">
            <Label htmlFor="qwen-base-url">
              {t("qwen.baseUrl")}
              {config.authType === "anthropic" ? " *" : ""}
            </Label>
            <Input
              id="qwen-base-url"
              value={config.baseUrl ?? ""}
              onChange={(e) => update({ ...config, baseUrl: e.target.value })}
              placeholder="https://api.example.com/v1"
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
          {config.envKey && (
            <div className="space-y-2">
              <Label htmlFor="qwen-env-key">{t("qwen.envKey")}</Label>
              <Input id="qwen-env-key" value={config.envKey} readOnly />
              <p className="text-xs text-fg-2">{t("qwen.envKeyHint")}</p>
            </div>
          )}
          <div className="space-y-3">
            <div className="flex items-center justify-between">
              <Label>{t("qwen.models")}</Label>
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={() =>
                  update({ ...config, models: [...config.models, { id: "" }] })
                }
              >
                <Plus className="mr-1 h-4 w-4" />
                {t("qwen.addModel")}
              </Button>
            </div>
            {problems.noModels && (
              <p className="text-xs text-destructive">
                {t("qwen.modelsRequired")}
              </p>
            )}
            {config.models.map((model, index) => {
              const duplicateId = problems.duplicateModelIds.has(
                model.id.trim(),
              );
              const size = model.generationConfig?.contextWindowSize;
              return (
                <div
                  key={index}
                  className="space-y-3 rounded-lg border border-border p-4"
                >
                  <div className="flex items-center justify-between gap-2">
                    <span className="truncate text-sm text-fg-2">
                      {model.name || model.id || `#${index + 1}`}
                    </span>
                    <Button
                      type="button"
                      variant="ghost"
                      size="icon"
                      aria-label={t("qwen.removeModel")}
                      onClick={() =>
                        update({
                          ...config,
                          models: config.models.filter((_, i) => i !== index),
                        })
                      }
                    >
                      <Trash2 className="h-4 w-4" />
                    </Button>
                  </div>
                  <div className="grid grid-cols-3 gap-3">
                    <div className="space-y-2">
                      <Label htmlFor={`qwen-model-id-${index}`}>
                        {t("qwen.modelId")} *
                      </Label>
                      <Input
                        id={`qwen-model-id-${index}`}
                        value={model.id}
                        onChange={(e) =>
                          updateModel(index, { ...model, id: e.target.value })
                        }
                      />
                    </div>
                    <div className="space-y-2">
                      <Label htmlFor={`qwen-model-name-${index}`}>
                        {t("qwen.modelName")}
                      </Label>
                      <Input
                        id={`qwen-model-name-${index}`}
                        value={model.name ?? ""}
                        onChange={(e) =>
                          updateModel(index, {
                            ...model,
                            name: e.target.value,
                          })
                        }
                      />
                    </div>
                    <div className="space-y-2">
                      <Label htmlFor={`qwen-context-${index}`}>
                        {t("qwen.contextWindowSize")}
                      </Label>
                      <Input
                        id={`qwen-context-${index}`}
                        type="number"
                        min={1}
                        step={1}
                        value={size ?? ""}
                        onChange={(e) =>
                          setContextWindow(index, e.target.value)
                        }
                      />
                    </div>
                  </div>
                  {duplicateId && (
                    <p className="text-xs text-destructive">
                      {t("qwen.duplicateModelId")}
                    </p>
                  )}
                </div>
              );
            })}
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
