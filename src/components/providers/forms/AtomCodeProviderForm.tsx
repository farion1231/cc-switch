import { useEffect, useState } from "react";
import { z } from "zod";
import { useForm } from "react-hook-form";
import { useTranslation } from "react-i18next";
import { Plus, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Form } from "@/components/ui/form";
import { ImeSafeInput } from "@/components/ui/ime-safe-input";
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
import {
  ATOMCODE_PROVIDER_TYPES,
  atomCodeCustomTemplate,
  atomcodeProviderPresets,
  type AtomCodeProviderPreset,
  type AtomCodeProviderType,
} from "@/config/atomcodeProviderPresets";
import { BasicFormFields } from "./BasicFormFields";
import ApiKeyInput from "./ApiKeyInput";
import { ProviderPresetSelector } from "./ProviderPresetSelector";
import type { ProviderFormProps } from "./ProviderForm";

type AtomCodeConfig = AtomCodeProviderPreset["settingsConfig"];

const configSchema = z
  .object({
    account: z
      .object({
        provider: z.string(),
        base_url: z.string(),
        api_key: z.string().optional(),
      })
      .passthrough(),
    models: z.record(
      z.string(),
      z
        .object({
          model: z.string(),
          context_window: z.number().optional(),
        })
        .passthrough(),
    ),
  })
  .passthrough();

// 与后端 atomcode_config::validate_provider 同款：字母、数字、`-`、`_`、`.`
const KEY_PATTERN = /^[a-zA-Z0-9._-]+$/;
const normalizeKey = (value: string) => value.replace(/[^a-zA-Z0-9._-]/g, "");

export function AtomCodeProviderForm({
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
  const isEdit = Boolean(initialData);
  const [config, setConfig] = useState<AtomCodeConfig>(() => {
    const raw = initialData?.settingsConfig as
      | Partial<AtomCodeConfig>
      | undefined;
    const template = atomCodeCustomTemplate();
    return {
      account: {
        provider: raw?.account?.provider ?? template.account.provider,
        base_url: raw?.account?.base_url ?? template.account.base_url,
        api_key: raw?.account?.api_key ?? template.account.api_key,
      },
      models: raw?.models && typeof raw.models === "object" ? raw.models : {},
    };
  });
  const [jsonText, setJsonText] = useState(() =>
    JSON.stringify(config, null, 2),
  );
  const [jsonValid, setJsonValid] = useState(true);
  const [presetId, setPresetId] = useState("custom");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  // 账号键即 config.toml 的 provider_accounts.<id>，供应商存在后固定
  const [providerKey, setProviderKey] = useState(providerId ?? "");
  const { data: existing } = useProvidersQuery("atomcode");
  const keyTaken =
    !isEdit &&
    Object.prototype.hasOwnProperty.call(
      existing?.providers ?? {},
      providerKey,
    );
  const keyInvalid =
    !isEdit && providerKey !== "" && !KEY_PATTERN.test(providerKey);
  const preset =
    presetId === "custom"
      ? undefined
      : atomcodeProviderPresets[Number(presetId)];
  const category = initialData?.category ?? preset?.category ?? "custom";

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
  const hasModels = Object.keys(config.models ?? {}).length > 0;
  const modelsValid = Object.entries(config.models ?? {}).every(
    ([key, model]) => key.trim() && model?.model?.trim(),
  );
  const ready = Boolean(
    jsonValid &&
      name.trim() &&
      (isEdit || (providerKey && !keyInvalid && !keyTaken)) &&
      hasModels &&
      modelsValid,
  );
  useEffect(() => {
    onSubmitReadyChange?.(ready);
  }, [ready, onSubmitReadyChange]);

  const update = (next: AtomCodeConfig, text?: string) => {
    setConfig(next);
    setJsonText(text ?? JSON.stringify(next, null, 2));
    setJsonValid(true);
  };
  const updateAccount = (next: Partial<AtomCodeConfig["account"]>) =>
    update({
      ...config,
      account: {
        ...(config.account ?? {}),
        ...next,
      } as AtomCodeConfig["account"],
    });

  const choosePreset = (id: string) => {
    setPresetId(id);
    const selected =
      id === "custom" ? undefined : atomcodeProviderPresets[Number(id)];
    const next: AtomCodeConfig = selected?.settingsConfig
      ? JSON.parse(JSON.stringify(selected.settingsConfig))
      : atomCodeCustomTemplate();
    update(next);
    setProviderKey(selected?.providerKey ?? "");
    form.reset({
      name: selected?.name ?? "",
      notes: "",
      websiteUrl: selected?.websiteUrl ?? "",
      icon: selected?.icon ?? "",
      iconColor: selected?.iconColor ?? "",
      settingsConfig: JSON.stringify(next, null, 2),
    });
  };

  const addModel = () => {
    const models = { ...config.models };
    let index = 1;
    while (models[`model-${index}`]) index += 1;
    models[`model-${index}`] = { model: "" };
    update({ ...config, models });
  };
  const removeModel = (key: string) => {
    const models = { ...config.models };
    delete models[key];
    update({ ...config, models });
  };
  const updateModel = (
    oldKey: string,
    next: { key?: string; model?: string; contextWindow?: string },
  ) => {
    const models: AtomCodeConfig["models"] = {};
    for (const [key, model] of Object.entries(config.models)) {
      if (key !== oldKey) {
        models[key] = model;
        continue;
      }
      const entry: AtomCodeConfig["models"][string] = { ...model };
      if (next.key !== undefined && next.key !== oldKey) {
        // 改键：按新键重排（保持行序，交给后面统一插入）
        models[next.key] = entry;
        continue;
      }
      if (next.model !== undefined) entry.model = next.model;
      if (next.contextWindow !== undefined) {
        const size = Number(next.contextWindow);
        if (
          next.contextWindow.trim() === "" ||
          !Number.isFinite(size) ||
          size <= 0
        )
          delete entry.context_window;
        else entry.context_window = size;
      }
      models[key] = entry;
    }
    update({ ...config, models });
  };

  const presetEntries = atomcodeProviderPresets.map((entry, index) => ({
    id: String(index),
    preset: entry,
  }));
  const presetSelector = !initialData && (
    <ProviderPresetSelector
      selectedPresetId={presetId}
      presetEntries={presetEntries}
      presetCategoryLabels={{
        custom: t("providerPreset.custom"),
        cn_official: t("providerForm.categoryCnOfficial"),
        aggregator: t("providerForm.categoryAggregation"),
        third_party: t("providerForm.categoryThirdParty"),
      }}
      onPresetChange={choosePreset}
      category={category}
    />
  );

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
            await onSubmit({
              ...identity,
              name: identity.name.trim(),
              meta: initialData?.meta,
              providerKey: isEdit ? providerId : providerKey,
              presetCategory: category,
              settingsConfig: JSON.stringify(config),
            });
          } catch (submitError) {
            setError(String(submitError));
          } finally {
            setBusy(false);
            onSubmittingChange?.(false);
          }
        })}
      >
        {presetSelector}
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
                <Label htmlFor="atomcode-provider-key">
                  {t("opencode.providerKey")}
                  <span aria-hidden="true" className="text-destructive ml-1">
                    *
                  </span>
                </Label>
                <ImeSafeInput
                  id="atomcode-provider-key"
                  value={providerKey}
                  onValueChange={setProviderKey}
                  normalize={normalizeKey}
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
                      ? t("atomcode.form.providerKeyInvalid")
                      : isEdit
                        ? t("opencode.providerKeyLockedHint")
                        : t("atomcode.form.providerKeyHint")}
                </p>
              </div>
            }
          />
          <div className="grid gap-4 sm:grid-cols-2">
            <div className="space-y-2">
              <Label htmlFor="atomcode-provider-type">
                {t("atomcode.form.providerType")}
              </Label>
              <Select
                value={config.account?.provider ?? "openai"}
                onValueChange={(value) =>
                  updateAccount({ provider: value as AtomCodeProviderType })
                }
              >
                <SelectTrigger id="atomcode-provider-type">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {ATOMCODE_PROVIDER_TYPES.map((type) => (
                    <SelectItem key={type.value} value={type.value}>
                      {type.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <div className="space-y-2">
              <Label htmlFor="atomcode-base-url">
                {t("atomcode.form.baseUrl")}
              </Label>
              <ImeSafeInput
                id="atomcode-base-url"
                value={config.account?.base_url ?? ""}
                onValueChange={(base_url) => updateAccount({ base_url })}
                placeholder="https://api.example.com/v1"
                autoComplete="off"
              />
              <p className="text-xs text-fg-2">
                {t("atomcode.form.baseUrlHint")}
              </p>
            </div>
            <div className="sm:col-span-2">
              <ApiKeyInput
                id="atomcode-api-key"
                label={t("atomcode.form.apiKey")}
                value={config.account?.api_key ?? ""}
                onChange={(api_key) => updateAccount({ api_key })}
                placeholder={t("atomcode.form.apiKeyPlaceholder")}
                hint={t("atomcode.form.apiKeyHint")}
              />
            </div>
          </div>
          <div className="space-y-2">
            <div className="flex items-center justify-between">
              <Label>
                {t("atomcode.form.models")}
                <span aria-hidden="true" className="text-destructive ml-1">
                  *
                </span>
              </Label>
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={addModel}
              >
                <Plus className="mr-1 h-4 w-4" />
                {t("atomcode.form.addModel")}
              </Button>
            </div>
            {!hasModels && (
              <p className="text-xs text-fg-2">{t("atomcode.form.noModels")}</p>
            )}
            <div className="space-y-2">
              {Object.entries(config.models ?? {}).map(([key, model]) => (
                <div key={key} className="flex items-start gap-2">
                  <div className="w-44 shrink-0">
                    <ImeSafeInput
                      aria-label={t("atomcode.form.modelKey")}
                      value={key}
                      onValueChange={(next) =>
                        updateModel(key, { key: normalizeKey(next) })
                      }
                      placeholder={t("atomcode.form.modelKeyPlaceholder")}
                      autoComplete="off"
                    />
                  </div>
                  <div className="min-w-0 flex-1">
                    <ImeSafeInput
                      aria-label={t("atomcode.form.modelId")}
                      value={model?.model ?? ""}
                      onValueChange={(next) =>
                        updateModel(key, { model: next })
                      }
                      placeholder="deepseek-flash"
                      autoComplete="off"
                    />
                  </div>
                  <div className="w-36 shrink-0">
                    <ImeSafeInput
                      aria-label={t("atomcode.form.contextWindow")}
                      value={model?.context_window?.toString() ?? ""}
                      onValueChange={(next) =>
                        updateModel(key, { contextWindow: next })
                      }
                      placeholder={t("atomcode.form.contextWindowPlaceholder")}
                      autoComplete="off"
                    />
                  </div>
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon"
                    aria-label={t("atomcode.form.removeModel")}
                    onClick={() => removeModel(key)}
                  >
                    <Trash2 className="h-4 w-4" />
                  </Button>
                </div>
              ))}
            </div>
          </div>
          <div className="space-y-2">
            <Label htmlFor="atomcode-settings-config">
              {t("provider.configJson")}
            </Label>
            <JsonEditor
              id="atomcode-settings-config"
              ariaLabel={t("provider.configJson")}
              value={jsonText}
              darkMode={isDarkMode}
              language="json"
              showValidation
              height={Math.max(1, jsonText.split("\n").length) * 20 + 20}
              onChange={(text) => {
                setJsonText(text);
                try {
                  const next = configSchema.parse(JSON.parse(text));
                  setConfig(next as unknown as AtomCodeConfig);
                  setJsonValid(true);
                } catch {
                  setJsonValid(false);
                }
              }}
            />
          </div>
        </fieldset>
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
