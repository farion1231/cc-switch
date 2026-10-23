import { useEffect, useState } from "react";
import { z } from "zod";
import { useForm } from "react-hook-form";
import { useTranslation } from "react-i18next";
import { Plus, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Form } from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import JsonEditor from "@/components/JsonEditor";
import { useDarkMode } from "@/hooks/useDarkMode";
import { useProvidersQuery } from "@/lib/query/queries";
import type { ProviderFormData } from "@/lib/schemas/provider";
import {
  WORKBUDDY_DEFAULT_CONFIG,
  workbuddyProviderPresets,
  type WorkBuddyModel,
  type WorkBuddySettingsConfig,
} from "@/config/workbuddyProviderPresets";
import ApiKeyInput from "./ApiKeyInput";
import { BasicFormFields } from "./BasicFormFields";
import { ProviderPresetSelector } from "./ProviderPresetSelector";
import type { ProviderFormProps } from "./ProviderForm";

const PRESET_ENTRIES = workbuddyProviderPresets.map((preset, index) => ({
  id: String(index),
  preset,
}));

const configSchema = z
  .object({
    vendor: z.string().optional(),
    url: z.string(),
    apiKey: z.string(),
    api: z.literal("openai-responses").optional(),
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

const CAPABILITIES = [
  "supportsToolCall",
  "supportsImages",
  "supportsReasoning",
] as const;

const parseTokens = (value: string): number | undefined => {
  const parsed = Number.parseInt(value, 10);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : undefined;
};

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
  const ready = Boolean(
    jsonValid &&
      name.trim() &&
      (isEdit || (providerKey && !keyInvalid && !keyTaken)) &&
      config.url?.trim() &&
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
  const updateModel = (index: number, patch: Partial<WorkBuddyModel>) =>
    update({
      ...config,
      models: models.map((model, i) =>
        i === index ? { ...model, ...patch } : model,
      ),
    });

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
                <Label htmlFor="workbuddy-provider-key">
                  {t("opencode.providerKey")}
                  <span aria-hidden="true" className="text-destructive ml-1">
                    *
                  </span>
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

          <div className="space-y-2">
            <Label htmlFor="workbuddy-url">
              {t("workbuddy.form.url")}
              <span aria-hidden="true" className="text-destructive ml-1">
                *
              </span>
            </Label>
            <Input
              id="workbuddy-url"
              value={config.url}
              onChange={(event) =>
                update({ ...config, url: event.target.value })
              }
              placeholder="https://api.example.com/v1/chat/completions"
              autoComplete="off"
            />
            <p className="text-xs text-fg-2">
              {config.api === "openai-responses"
                ? t("workbuddy.form.urlResponsesHint")
                : t("workbuddy.form.urlHint")}
            </p>
          </div>

          <div className="flex items-center justify-between gap-4">
            <div className="space-y-1">
              <Label htmlFor="workbuddy-responses">
                {t("workbuddy.form.responsesApi")}
              </Label>
              <p className="text-xs text-fg-2">
                {t("workbuddy.form.responsesApiHint")}
              </p>
            </div>
            <Switch
              id="workbuddy-responses"
              checked={config.api === "openai-responses"}
              onCheckedChange={(checked) => {
                const next = { ...config };
                if (checked) next.api = "openai-responses";
                else delete next.api;
                update(next);
              }}
            />
          </div>

          <ApiKeyInput
            id="workbuddy-api-key"
            value={config.apiKey}
            onChange={(apiKey) => update({ ...config, apiKey })}
            hint={t("workbuddy.form.apiKeyHint")}
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

          <div className="space-y-3">
            <div className="flex items-center justify-between">
              <Label>
                {t("workbuddy.form.models")}
                <span aria-hidden="true" className="text-destructive ml-1">
                  *
                </span>
              </Label>
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={() =>
                  update({
                    ...config,
                    models: [
                      ...models,
                      { id: "", supportsToolCall: true, supportsImages: false },
                    ],
                  })
                }
              >
                <Plus className="h-4 w-4 mr-1" />
                {t("workbuddy.form.addModel")}
              </Button>
            </div>
            {models.length === 0 && (
              <p className="text-xs text-fg-2">
                {t("workbuddy.form.modelsEmpty")}
              </p>
            )}
            {models.map((model, index) => (
              <div
                key={index}
                className="space-y-3 rounded-md border border-border p-3"
              >
                <div className="grid grid-cols-2 gap-3">
                  <Input
                    aria-label={t("workbuddy.form.modelId")}
                    value={model.id}
                    onChange={(event) =>
                      updateModel(index, { id: event.target.value })
                    }
                    placeholder={t("workbuddy.form.modelId")}
                    autoComplete="off"
                  />
                  <div className="flex gap-2">
                    <Input
                      aria-label={t("workbuddy.form.modelName")}
                      value={model.name ?? ""}
                      onChange={(event) =>
                        updateModel(index, {
                          name: event.target.value || undefined,
                        })
                      }
                      placeholder={t("workbuddy.form.modelName")}
                      autoComplete="off"
                    />
                    <Button
                      type="button"
                      variant="ghost"
                      size="icon"
                      aria-label={t("workbuddy.form.removeModel")}
                      onClick={() =>
                        update({
                          ...config,
                          models: models.filter((_, i) => i !== index),
                        })
                      }
                    >
                      <Trash2 className="h-4 w-4" />
                    </Button>
                  </div>
                  <Input
                    aria-label={t("workbuddy.form.maxInputTokens")}
                    inputMode="numeric"
                    value={model.maxInputTokens ?? ""}
                    onChange={(event) =>
                      updateModel(index, {
                        maxInputTokens: parseTokens(event.target.value),
                      })
                    }
                    placeholder={t("workbuddy.form.maxInputTokens")}
                  />
                  <Input
                    aria-label={t("workbuddy.form.maxOutputTokens")}
                    inputMode="numeric"
                    value={model.maxOutputTokens ?? ""}
                    onChange={(event) =>
                      updateModel(index, {
                        maxOutputTokens: parseTokens(event.target.value),
                      })
                    }
                    placeholder={t("workbuddy.form.maxOutputTokens")}
                  />
                </div>
                <div className="flex flex-wrap gap-4">
                  {CAPABILITIES.map((capability) => (
                    <label
                      key={capability}
                      className="flex items-center gap-2 text-xs text-fg-2"
                    >
                      <Switch
                        checked={model[capability] === true}
                        onCheckedChange={(checked) =>
                          updateModel(index, { [capability]: checked })
                        }
                      />
                      {t(`workbuddy.form.${capability}`)}
                    </label>
                  ))}
                </div>
              </div>
            ))}
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
