import { useEffect, useState } from "react";
import { z } from "zod";
import { useForm } from "react-hook-form";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Form } from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import JsonEditor from "@/components/JsonEditor";
import { useDarkMode } from "@/hooks/useDarkMode";
import { useProvidersQuery } from "@/lib/query/queries";
import type { ProviderFormData } from "@/lib/schemas/provider";
import type { OpenCodeModel } from "@/types";
import {
  commandcodeProviderPresets,
  type CommandCodeApiFormat,
  type CommandCodeProviderConfig,
  type CommandCodeProviderModel,
} from "@/config/commandcodeProviderPresets";
import { commandcodePresetModelSources } from "@/config/presetModelMetadata";
import { BasicFormFields } from "./BasicFormFields";
import { OpenCodeFormFields } from "./OpenCodeFormFields";
import { ProviderPresetSelector } from "./ProviderPresetSelector";
import { normalizeRequestHeaders } from "./helpers/requestHeaders";
import type { ProviderFormProps } from "./ProviderForm";

const API_FORMATS = [
  { value: "openai-completions", label: "OpenAI Chat Completions" },
  { value: "openai-responses", label: "OpenAI Responses" },
  { value: "anthropic-messages", label: "Anthropic Messages" },
];
const PRESET_ENTRIES = commandcodeProviderPresets.map((preset, index) => ({
  id: String(index),
  preset,
}));
// `openai-completions` is Command Code's default and stays out of the file.
const DEFAULT_API_FORMAT = "openai-completions";
const configSchema = z
  .object({
    name: z.string().optional(),
    api: z.string().optional(),
    baseURL: z.string().optional(),
    apiKey: z.union([z.string(), z.literal(false)]).optional(),
    headers: z.record(z.string(), z.string()).optional(),
    models: z
      .record(
        z.string(),
        z
          .object({
            name: z.string().optional(),
            contextWindow: z.number().optional(),
            maxOutput: z.number().optional(),
            limit: z
              .object({
                context: z.number().optional(),
                output: z.number().optional(),
              })
              .passthrough()
              .optional(),
          })
          .passthrough(),
      )
      .optional(),
  })
  .passthrough();
/** Command Code writes `contextWindow` / `maxOutput`; the editor works in `limit`. */
type CommandCodeFormConfig = {
  name?: string;
  api?: string;
  baseURL?: string;
  apiKey?: string | false;
  headers?: Record<string, string>;
  models: Record<string, OpenCodeModel>;
  [key: string]: unknown;
};
const EMPTY_CONFIG: CommandCodeFormConfig = { baseURL: "", models: {} };

const toFormModel = (value: unknown): OpenCodeModel => {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    return { name: "" };
  }
  const { contextWindow, maxOutput, limit, ...rest } =
    value as CommandCodeProviderModel;
  const aliases =
    limit && typeof limit === "object"
      ? (limit as { context?: unknown; output?: unknown })
      : undefined;
  const context =
    contextWindow ??
    (typeof aliases?.context === "number" ? aliases.context : undefined);
  const output =
    maxOutput ??
    (typeof aliases?.output === "number" ? aliases.output : undefined);
  return {
    ...rest,
    ...(context !== undefined || output !== undefined
      ? {
          limit: {
            ...(context !== undefined ? { context } : {}),
            ...(output !== undefined ? { output } : {}),
          },
        }
      : {}),
  } as OpenCodeModel;
};

const toFormConfig = (value: unknown): CommandCodeFormConfig => {
  const source =
    value && typeof value === "object" && !Array.isArray(value)
      ? (value as Record<string, unknown>)
      : {};
  const rawModels =
    source.models &&
    typeof source.models === "object" &&
    !Array.isArray(source.models)
      ? (source.models as Record<string, unknown>)
      : {};
  return {
    ...source,
    ...(typeof source.name === "string" ? { name: source.name } : {}),
    ...(typeof source.api === "string" ? { api: source.api } : {}),
    ...(typeof source.baseURL === "string" ? { baseURL: source.baseURL } : {}),
    ...(source.apiKey === false || typeof source.apiKey === "string"
      ? { apiKey: source.apiKey }
      : {}),
    ...(source.headers &&
    typeof source.headers === "object" &&
    !Array.isArray(source.headers)
      ? { headers: source.headers as Record<string, string> }
      : {}),
    models: Object.fromEntries(
      Object.entries(rawModels).map(([id, model]) => [id, toFormModel(model)]),
    ),
  };
};

const toNativeModel = (model: OpenCodeModel): CommandCodeProviderModel => {
  const { limit, ...rest } = model;
  return {
    ...rest,
    ...(typeof limit?.context === "number"
      ? { contextWindow: limit.context }
      : {}),
    ...(typeof limit?.output === "number" ? { maxOutput: limit.output } : {}),
  };
};

const toNativeConfig = (
  config: CommandCodeFormConfig,
  name: string,
): CommandCodeProviderConfig => {
  const { api, apiKey, models, headers, ...rest } = config;
  const normalizedHeaders = normalizeRequestHeaders(headers ?? {});
  const key = typeof apiKey === "string" ? apiKey.trim() : "";
  return {
    ...rest,
    ...(name ? { name } : {}),
    ...(api && api !== DEFAULT_API_FORMAT
      ? { api: api as CommandCodeApiFormat }
      : {}),
    baseURL: config.baseURL ?? "",
    // `apiKey: false` is Command Code's marker for a keyless endpoint.
    apiKey: key || false,
    ...(Object.keys(normalizedHeaders).length
      ? { headers: normalizedHeaders }
      : {}),
    models: Object.fromEntries(
      Object.entries(models ?? {}).map(([id, model]) => [
        id,
        toNativeModel(model),
      ]),
    ),
  };
};

const serializeConfig = (config: CommandCodeFormConfig) =>
  JSON.stringify(toNativeConfig(config, config.name ?? ""), null, 2);
const KEY_PATTERN = /^[a-z0-9][a-z0-9_.-]{0,63}$/;
const normalizeKey = (value: string) =>
  value.toLowerCase().replace(/[^a-z0-9_.-]/g, "");
// Ids the CLI reserves for its built-in subscription lanes and account fields.
const RESERVED_PROVIDER_IDS = new Set([
  "anthropic",
  "github-copilot",
  "codex",
  "apikey",
  "userid",
  "username",
  "keyname",
  "authenticatedat",
]);

export function CommandCodeProviderForm({
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
  const [config, setConfig] = useState<CommandCodeFormConfig>(() =>
    initialData?.settingsConfig
      ? toFormConfig(initialData.settingsConfig)
      : EMPTY_CONFIG,
  );
  const [jsonText, setJsonText] = useState(() => serializeConfig(config));
  const [jsonValid, setJsonValid] = useState(true);
  const [presetId, setPresetId] = useState("custom");
  const preset =
    presetId === "custom"
      ? undefined
      : commandcodeProviderPresets[Number(presetId)];
  const category = initialData?.category ?? preset?.category ?? "custom";
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  // The provider id is the `providers.json` key; Command Code resolves keys and
  // stored credentials by it, so it is fixed once the provider exists.
  const isEdit = Boolean(initialData);
  const [providerKey, setProviderKey] = useState(providerId ?? "");
  const { data: existing } = useProvidersQuery("commandcode");
  const keyTaken =
    !isEdit &&
    Object.prototype.hasOwnProperty.call(
      existing?.providers ?? {},
      providerKey,
    );
  const keyInvalid =
    !isEdit &&
    providerKey !== "" &&
    (!KEY_PATTERN.test(providerKey) || RESERVED_PROVIDER_IDS.has(providerKey));
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
  const ready = Boolean(
    jsonValid &&
      name.trim() &&
      (isEdit || (providerKey && !keyInvalid && !keyTaken)) &&
      config.baseURL?.trim() &&
      Object.keys(config.models ?? {}).length,
  );
  useEffect(() => {
    onSubmitReadyChange?.(ready);
  }, [ready, onSubmitReadyChange]);
  const update = (next: CommandCodeFormConfig) => {
    setConfig(next);
    setJsonText(serializeConfig(next));
    setJsonValid(true);
  };
  const choosePreset = (id: string) => {
    setPresetId(id);
    const selected =
      id === "custom" ? undefined : commandcodeProviderPresets[Number(id)];
    const next = selected
      ? { ...selected.settingsConfig }
      : { baseURL: "", models: {} };
    update(toFormConfig(next));
    setProviderKey(selected?.providerKey ?? "");
    form.reset({
      name: selected
        ? selected.nameKey
          ? t(selected.nameKey)
          : selected.name
        : "",
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
            await onSubmit({
              ...identity,
              name: identity.name.trim(),
              meta: initialData?.meta,
              providerKey: isEdit ? providerId : providerKey,
              presetCategory: category,
              settingsConfig: JSON.stringify(
                toNativeConfig(config, identity.name.trim()),
              ),
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
                <Label htmlFor="commandcode-provider-key">
                  {t("opencode.providerKey")}
                  <span aria-hidden="true" className="text-destructive ml-1">
                    *
                  </span>
                </Label>
                <Input
                  id="commandcode-provider-key"
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
                        ? t("opencode.providerKeyLockedHint")
                        : t("commandcode.providerKeyHint")}
                </p>
              </div>
            }
          />
          <OpenCodeFormFields
            showExtraOptions={false}
            apiFormats={API_FORMATS}
            presetModelSources={commandcodePresetModelSources}
            npm={config.api ?? DEFAULT_API_FORMAT}
            onNpmChange={(api) => update({ ...config, api })}
            apiKey={typeof config.apiKey === "string" ? config.apiKey : ""}
            onApiKeyChange={(apiKey) => update({ ...config, apiKey })}
            category={category}
            shouldShowApiKeyLink={Boolean(preset?.apiKeyUrl)}
            websiteUrl={preset?.apiKeyUrl ?? ""}
            isPartner={preset?.isPartner}
            partnerPromotionKey={preset?.partnerPromotionKey}
            baseUrl={config.baseURL ?? ""}
            onBaseUrlChange={(baseURL) => update({ ...config, baseURL })}
            headers={config.headers ?? {}}
            onHeadersChange={(headers) => update({ ...config, headers })}
            models={(config.models ?? {}) as Record<string, OpenCodeModel>}
            onModelsChange={(models) => update({ ...config, models })}
          />
        </fieldset>
        <div className="space-y-2">
          <Label htmlFor="commandcode-settings-config">
            {t("provider.configJson")}
          </Label>
          <JsonEditor
            id="commandcode-settings-config"
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
                setConfig(toFormConfig(next));
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
