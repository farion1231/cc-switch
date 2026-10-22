import { useEffect, useState } from "react";
import { useForm } from "react-hook-form";
import { useTranslation } from "react-i18next";
import { Plus, Trash2 } from "lucide-react";
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
import { useProvidersQuery } from "@/lib/query/queries";
import type { ProviderFormData } from "@/lib/schemas/provider";
import type { ProviderCategory } from "@/types";
import {
  emptyKimiConfig,
  kimiConfigSchema,
  kimiProviderPresets,
  KIMI_PROVIDER_TYPES,
  KIMI_CAPABILITIES,
  type KimiProviderConfig,
} from "@/config/kimiProviderPresets";
import { normalizeRequestHeaders } from "./helpers/requestHeaders";
import { BasicFormFields } from "./BasicFormFields";
import { ApiKeySection } from "./shared";
import { RequestHeadersEditor } from "./RequestHeadersEditor";
import { ProviderPresetSelector } from "./ProviderPresetSelector";
import type { ProviderFormProps } from "./ProviderForm";

const PRESETS = kimiProviderPresets.map((preset, index) => ({
  id: String(index),
  preset,
}));

export function KimiProviderForm({
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
  const update = (next: KimiProviderConfig) => {
    setConfig(next);
    setText(JSON.stringify(next, null, 2));
    setJsonValid(true);
  };
  const updateProvider = (patch: Partial<KimiProviderConfig["provider"]>) =>
    update({ ...config, provider: { ...config.provider, ...patch } });
  const updateModel = (
    key: string,
    patch: Partial<KimiProviderConfig["models"][string]>,
  ) =>
    update({
      ...config,
      models: { ...config.models, [key]: { ...config.models[key], ...patch } },
    });
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
              const preset =
                id === "custom" ? undefined : kimiProviderPresets[Number(id)];
              setCategory(preset?.category ?? "custom");
              update(
                kimiConfigSchema.parse(
                  structuredClone(preset?.settingsConfig ?? emptyKimiConfig()),
                ),
              );
              setProviderKey(preset?.providerKey ?? "");
              form.reset({
                name: preset?.nameKey
                  ? t(preset.nameKey)
                  : (preset?.name ?? ""),
                notes: "",
                websiteUrl: preset?.websiteUrl ?? "",
                icon: preset?.icon ?? "",
                iconColor: preset?.iconColor ?? "",
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
                    const models = Object.fromEntries(
                      Object.entries(config.models).map(([key, model]) => [
                        key.startsWith(`${providerKey}/`)
                          ? `${next}/${key.slice(providerKey.length + 1)}`
                          : key,
                        model,
                      ]),
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
          <RequestHeadersEditor
            headers={config.provider.custom_headers ?? {}}
            onHeadersChange={(custom_headers) =>
              updateProvider({ custom_headers })
            }
          />
          <div className="space-y-3">
            <div className="flex items-center justify-between">
              <Label>{t("kimi.models")}</Label>
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={() => {
                  let index = Object.keys(config.models).length + 1;
                  let key = `${providerKey || "custom"}/model-${index}`;
                  while (
                    Object.prototype.hasOwnProperty.call(config.models, key)
                  )
                    key = `${providerKey || "custom"}/model-${++index}`;
                  update({
                    ...config,
                    models: {
                      ...config.models,
                      [key]: { model: "", max_context_size: 262144 },
                    },
                  });
                }}
              >
                <Plus className="mr-1 h-4 w-4" />
                {t("kimi.addModel")}
              </Button>
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
                    <Input
                      id={`kimi-model-${key}`}
                      value={model.model}
                      onChange={(e) =>
                        updateModel(key, { model: e.target.value })
                      }
                    />
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
                      onChange={(e) =>
                        updateModel(key, {
                          max_context_size: Number(e.target.value),
                        })
                      }
                    />
                  </div>
                </div>
                <div className="flex flex-wrap gap-4">
                  {KIMI_CAPABILITIES.map((capability) => (
                    <label
                      key={capability}
                      className="flex items-center gap-2 text-sm"
                    >
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
        )}
      </form>
    </Form>
  );
}
