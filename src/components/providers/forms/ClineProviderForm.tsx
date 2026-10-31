import { useEffect, useMemo, useRef, useState } from "react";
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
import {
  fetchModelsForConfig,
  showFetchModelsError,
  type FetchedModel,
} from "@/lib/api/model-fetch";
import { useProvidersQuery } from "@/lib/query/queries";
import { toast } from "@/lib/toast";
import type { ProviderFormData } from "@/lib/schemas/provider";
import type { ProviderCategory } from "@/types";
import {
  CLINE_BUILTIN_MODEL_BASE_URLS,
  CLINE_PROTOCOLS,
  clineConfigSchema,
  clineProviderPresets,
  emptyClineCustomConfig,
  generateClineProviderId,
  isClineCustomConfig,
  validateClineConfig,
  withClineProtocol,
  type ClineModel,
  type ClineProviderConfig,
} from "@/config/clineProviderPresets";
import {
  findRequestHeaderValue,
  normalizeRequestHeaders,
} from "./helpers/requestHeaders";
import { BasicFormFields } from "./BasicFormFields";
import { ApiKeySection, ModelInputWithFetch } from "./shared";
import {
  FetchedModelPicker,
  type FetchedModelSelection,
} from "./FetchedModelPicker";
import { RequestHeadersEditor } from "./RequestHeadersEditor";
import { ProviderPresetSelector } from "./ProviderPresetSelector";
import type { ProviderFormProps } from "./ProviderForm";

const PRESETS = clineProviderPresets.map((preset, index) => ({
  id: String(index),
  preset,
}));

/** Model rows keep their order and tolerate blank / duplicate ids while typing. */
type ModelRow = { id: string; model: ClineModel };

const NUMBER_FIELDS = [
  ["contextWindow", "cline.contextWindow"],
  ["maxTokens", "cline.maxTokens"],
  ["inputPrice", "cline.inputPrice"],
  ["outputPrice", "cline.outputPrice"],
] as const;

function toRows(config: ClineProviderConfig): ModelRow[] {
  return Object.entries(config.models ?? {}).map(([id, model]) => ({
    id,
    model,
  }));
}

/** The full config as Cline will store it: rows become the `models` map. */
function assemble(
  config: ClineProviderConfig,
  rows: ModelRow[],
): ClineProviderConfig {
  if (!config.provider) {
    const { models: _models, provider: _provider, ...builtin } = config;
    return builtin;
  }
  return {
    ...config,
    models: Object.fromEntries(
      rows.map(({ id, model }) => [id.trim(), { ...model, id: id.trim() }]),
    ),
  };
}

export function ClineProviderForm({
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
  const initial = useMemo(() => {
    const parsed = clineConfigSchema.safeParse(
      initialData?.settingsConfig ?? emptyClineCustomConfig(),
    );
    return parsed.success ? parsed.data : emptyClineCustomConfig();
  }, [initialData?.settingsConfig]);
  const [config, setConfig] = useState<ClineProviderConfig>(initial);
  const [rows, setRows] = useState<ModelRow[]>(() => toRows(initial));
  const [text, setText] = useState(JSON.stringify(initial, null, 2));
  const [jsonValid, setJsonValid] = useState(true);
  const [presetId, setPresetId] = useState("custom");
  const [category, setCategory] = useState<ProviderCategory>(
    initialData?.category ?? "custom",
  );
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [issues, setIssues] = useState<string[]>([]);
  const [fetchedModels, setFetchedModels] = useState<FetchedModel[]>([]);
  const [isFetchingModels, setIsFetchingModels] = useState(false);
  const fetchGeneration = useRef(0);
  const { data: existing } = useProvidersQuery("cline");
  const isEdit = Boolean(initialData);
  const isCustom = isClineCustomConfig(config);
  // Cline keeps OAuth tokens itself; CC Switch only switches to such entries.
  const isOAuth = config.tokenSource === "oauth";
  const preset =
    presetId === "custom" ? undefined : clineProviderPresets[Number(presetId)];
  const existingIds = Object.keys(existing?.providers ?? {});
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
  // Custom ids follow the name until saved and never change afterwards;
  // built-in providers are Cline's own slots and keep their built-in id.
  const providerKey = isEdit
    ? (providerId ?? "")
    : isCustom
      ? generateClineProviderId(name, existingIds)
      : (preset?.providerKey ?? "");
  const builtinTaken =
    !isEdit && !isCustom && existingIds.includes(providerKey);
  const apiKeyRequired =
    !isEdit && !isOAuth && category !== "local" && !builtinTaken;
  const ready = Boolean(
    jsonValid &&
      name.trim() &&
      providerKey &&
      !builtinTaken &&
      (!apiKeyRequired || config.settings.apiKey?.trim()) &&
      (!isCustom || (config.provider?.baseUrl.trim() && rows.length > 0)),
  );
  useEffect(() => {
    onSubmitReadyChange?.(ready);
  }, [ready, onSubmitReadyChange]);

  const sync = (nextConfig: ClineProviderConfig, nextRows: ModelRow[]) => {
    setConfig(nextConfig);
    setRows(nextRows);
    setText(JSON.stringify(assemble(nextConfig, nextRows), null, 2));
    setJsonValid(true);
    setIssues([]);
  };
  const update = (next: ClineProviderConfig) => sync(next, rows);
  const updateSettings = (patch: Partial<ClineProviderConfig["settings"]>) =>
    update({ ...config, settings: { ...config.settings, ...patch } });
  const updateProvider = (
    patch: Partial<NonNullable<ClineProviderConfig["provider"]>>,
  ) =>
    config.provider &&
    update({ ...config, provider: { ...config.provider, ...patch } });
  const updateRows = (next: ModelRow[], selected = config.settings.model) => {
    const ids = next.map((row) => row.id);
    // Keep the selected model valid: follow renames, fall back to the first row.
    const model = ids.includes(selected ?? "") ? selected : ids[0];
    sync({ ...config, settings: { ...config.settings, model } }, next);
  };
  const updateRow = (index: number, patch: Partial<ModelRow>) => {
    const next = rows.map((row, i) =>
      i === index ? { ...row, ...patch } : row,
    );
    const renamed =
      patch.id !== undefined && rows[index].id === config.settings.model;
    updateRows(next, renamed ? patch.id : config.settings.model);
  };

  const presetSelector = (
    <ProviderPresetSelector
      selectedPresetId={presetId}
      presetEntries={PRESETS}
      category={category}
      onPresetChange={(id) => {
        setPresetId(id);
        const next =
          id === "custom" ? undefined : clineProviderPresets[Number(id)];
        setCategory(next?.category ?? "custom");
        const nextConfig = clineConfigSchema.parse(
          structuredClone(next?.settingsConfig ?? emptyClineCustomConfig()),
        );
        sync(nextConfig, toRows(nextConfig));
        setError("");
        fetchGeneration.current += 1;
        setFetchedModels([]);
        setIsFetchingModels(false);
        form.reset({
          name:
            next && next.category !== "custom"
              ? next.nameKey
                ? t(next.nameKey)
                : next.name
              : "",
          notes: "",
          websiteUrl: next?.websiteUrl ?? "",
          icon: next?.icon ?? "",
          iconColor: next?.iconColor ?? "",
          settingsConfig: "",
        });
      }}
    />
  );

  const headers = config.settings.headers ?? {};
  const selectedModel = config.settings.model ?? "";

  // Built-in providers fall back to Cline's default endpoint when it is known.
  const fetchBaseUrl = isCustom
    ? (config.provider?.baseUrl.trim() ?? "")
    : config.settings.baseUrl?.trim() ||
      CLINE_BUILTIN_MODEL_BASE_URLS[providerKey] ||
      "";
  const handleFetchModels = () => {
    const apiKey = config.settings.apiKey?.trim() ?? "";
    if (!fetchBaseUrl) {
      showFetchModelsError(null, t, {
        hasApiKey: Boolean(apiKey),
        hasBaseUrl: false,
      });
      return;
    }
    const requestHeaders = normalizeRequestHeaders(headers);
    const protocol = config.provider?.protocol;
    const generation = ++fetchGeneration.current;
    setFetchedModels([]);
    setIsFetchingModels(true);
    fetchModelsForConfig(
      fetchBaseUrl,
      apiKey,
      undefined,
      undefined,
      findRequestHeaderValue(requestHeaders, "user-agent"),
      {
        apiFormat:
          protocol === "anthropic"
            ? "anthropic-messages"
            : protocol === "gemini"
              ? "google-generative-ai"
              : undefined,
        requestHeaders: Object.keys(requestHeaders).length
          ? requestHeaders
          : undefined,
      },
    )
      .then((result) => {
        if (generation !== fetchGeneration.current) return;
        const models = [
          ...new Map(result.map((model) => [model.id, model])).values(),
        ];
        setFetchedModels(models);
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
  };
  const applyFetchedModels = ({ add, remove }: FetchedModelSelection) => {
    const removed = new Set(remove);
    updateRows([
      ...rows.filter((row) => !removed.has(row.id.trim())),
      ...add.map((id) => ({ id, model: { name: id } })),
    ]);
  };

  return (
    <Form {...form}>
      <form
        id="provider-form"
        className="space-y-6"
        onSubmit={form.handleSubmit(async (identity) => {
          if (!ready || busy) return;
          const trimmedName = identity.name.trim();
          const settings = {
            ...config.settings,
            ...(config.settings.baseUrl !== undefined
              ? { baseUrl: config.settings.baseUrl.trim() }
              : {}),
          };
          const normalized = normalizeRequestHeaders(headers);
          if (Object.keys(normalized).length) settings.headers = normalized;
          else delete settings.headers;
          if (!settings.baseUrl) delete settings.baseUrl;
          if (!isCustom && !settings.model) delete settings.model;
          const assembled = assemble({ ...config, settings }, rows);
          const payload = assembled;
          if (assembled.provider) {
            const provider = {
              ...assembled.provider,
              name: trimmedName,
              baseUrl: assembled.provider.baseUrl.trim(),
              defaultModelId: settings.model,
            };
            // Anthropic and Gemini need their matching client; other native
            // clients are left alone.
            payload.provider = provider.protocol
              ? withClineProtocol(provider, provider.protocol)
              : provider;
          }
          const found = validateClineConfig(payload);
          const ids = rows.map((row) => row.id.trim());
          if (new Set(ids).size !== ids.length) {
            found.push("cline.validation.duplicateModel");
          }
          if (found.length) {
            setIssues(found);
            return;
          }
          setBusy(true);
          onSubmittingChange?.(true);
          setError("");
          try {
            await onSubmit({
              ...identity,
              name: trimmedName,
              providerKey,
              meta: initialData?.meta,
              presetCategory: category,
              settingsConfig: JSON.stringify(payload),
            });
            // cline-hub loads models.json only when it starts.
            if (isCustom) toast.info(t("cline.restartHint"));
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
        {issues.length > 0 && (
          <ul
            role="alert"
            className="list-disc space-y-1 ps-5 text-sm text-destructive"
          >
            {issues.map((issue) => (
              <li key={issue}>{t(issue)}</li>
            ))}
          </ul>
        )}
        {isOAuth && (
          <p className="rounded-lg border border-border p-3 text-sm text-fg-2">
            {t("cline.oauthReadOnly")}
          </p>
        )}
        {isCustom && (
          <p className="text-sm text-fg-2">{t("cline.restartHint")}</p>
        )}
        <fieldset
          disabled={busy || !jsonValid}
          className="min-w-0 space-y-6 border-0 p-0 disabled:opacity-50"
        >
          <BasicFormFields
            form={form}
            beforeNameSlot={
              <div className="space-y-2">
                <Label htmlFor="cline-provider-key">
                  {t("cline.providerId")}
                </Label>
                <Input
                  id="cline-provider-key"
                  value={providerKey}
                  readOnly
                  disabled
                />
                <p
                  className={
                    builtinTaken
                      ? "text-xs text-destructive"
                      : "text-xs text-fg-2"
                  }
                >
                  {builtinTaken
                    ? t("cline.builtinExists", { id: providerKey })
                    : isEdit
                      ? t("opencode.providerKeyLockedHint")
                      : isCustom
                        ? t("cline.customIdHint")
                        : t("cline.builtinIdHint")}
                </p>
              </div>
            }
          />
          {isCustom && config.provider && (
            <>
              <div className="space-y-2">
                <Label htmlFor="cline-protocol">{t("cline.protocol")}</Label>
                <Select
                  value={config.provider.protocol ?? "openai-chat"}
                  onValueChange={(protocol) =>
                    config.provider &&
                    update({
                      ...config,
                      provider: withClineProtocol(config.provider, protocol),
                    })
                  }
                >
                  <SelectTrigger id="cline-protocol">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {[
                      ...CLINE_PROTOCOLS,
                      // A protocol set in Cline that is no longer offered stays visible.
                      ...(config.provider.protocol &&
                      !(CLINE_PROTOCOLS as readonly string[]).includes(
                        config.provider.protocol,
                      )
                        ? [config.provider.protocol]
                        : []),
                    ].map((protocol) => (
                      <SelectItem key={protocol} value={protocol}>
                        {protocol}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
              <div className="space-y-2">
                <Label htmlFor="cline-base-url">{t("cline.baseUrl")} *</Label>
                <Input
                  id="cline-base-url"
                  value={config.provider.baseUrl}
                  onChange={(e) => updateProvider({ baseUrl: e.target.value })}
                  placeholder="https://api.example.com/v1"
                />
                <p className="text-xs text-fg-2">{t("cline.baseUrlHint")}</p>
              </div>
            </>
          )}
          {!isOAuth && (
            <>
              {!isCustom && (
                <div className="space-y-2">
                  <Label htmlFor="cline-base-url">
                    {t("cline.baseUrlOverride")}
                  </Label>
                  <Input
                    id="cline-base-url"
                    value={config.settings.baseUrl ?? ""}
                    onChange={(e) =>
                      updateSettings({ baseUrl: e.target.value })
                    }
                    placeholder={t("cline.baseUrlOverridePlaceholder")}
                  />
                </div>
              )}
              <ApiKeySection
                id="cline-api-key"
                value={config.settings.apiKey ?? ""}
                onChange={(apiKey) => updateSettings({ apiKey })}
                category={category}
                shouldShowLink={Boolean(preset?.apiKeyUrl)}
                websiteUrl={preset?.apiKeyUrl ?? ""}
                isPartner={preset?.isPartner}
                partnerPromotionKey={preset?.partnerPromotionKey}
                required={apiKeyRequired}
              />
              <RequestHeadersEditor
                headers={headers}
                onHeadersChange={(next) => updateSettings({ headers: next })}
              />
            </>
          )}
          {!isCustom && (
            <div className="space-y-2">
              <Label htmlFor="cline-model">{t("cline.model")}</Label>
              {fetchBaseUrl && !isOAuth ? (
                <ModelInputWithFetch
                  id="cline-model"
                  value={selectedModel}
                  onChange={(model) => updateSettings({ model })}
                  fetchedModels={fetchedModels}
                  isLoading={isFetchingModels}
                  onFetch={handleFetchModels}
                />
              ) : (
                <>
                  <Input
                    id="cline-model"
                    list="cline-model-suggestions"
                    value={selectedModel}
                    onChange={(e) => updateSettings({ model: e.target.value })}
                  />
                  <datalist id="cline-model-suggestions">
                    {(preset?.suggestedModels ?? []).map((model) => (
                      <option key={model} value={model} />
                    ))}
                  </datalist>
                </>
              )}
              <p className="text-xs text-fg-2">{t("cline.builtinModelHint")}</p>
            </div>
          )}
          {isCustom && (
            <div className="space-y-3">
              <div className="flex items-center justify-between">
                <Label>{t("cline.models")} *</Label>
                <div className="flex gap-1">
                  {!isOAuth && (
                    <FetchedModelPicker
                      models={fetchedModels}
                      configuredModelIds={rows.map((row) => row.id)}
                      onApply={applyFetchedModels}
                      onFetch={handleFetchModels}
                      isFetching={isFetchingModels}
                    />
                  )}
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    className="h-7 gap-1"
                    onClick={() => updateRows([...rows, { id: "", model: {} }])}
                  >
                    <Plus className="h-3.5 w-3.5" />
                    {t("cline.addModel")}
                  </Button>
                </div>
              </div>
              {rows.map((row, index) => (
                <div
                  key={index}
                  className="space-y-3 rounded-lg border border-border p-4"
                >
                  <div className="flex items-center justify-between gap-2">
                    <label className="flex items-center gap-2 text-sm">
                      <input
                        type="radio"
                        name="cline-selected-model"
                        aria-label={t("cline.useModel", {
                          model: row.id || index + 1,
                        })}
                        checked={row.id === selectedModel}
                        onChange={() => updateRows(rows, row.id)}
                      />
                      {t("cline.selectedModel")}
                    </label>
                    <Button
                      type="button"
                      variant="ghost"
                      size="icon"
                      aria-label={t("cline.removeModel")}
                      onClick={() =>
                        updateRows(rows.filter((_, i) => i !== index))
                      }
                    >
                      <Trash2 className="h-4 w-4" />
                    </Button>
                  </div>
                  <div className="grid grid-cols-2 gap-3">
                    <div className="space-y-2">
                      <Label htmlFor={`cline-model-id-${index}`}>
                        {t("cline.modelId")} *
                      </Label>
                      <Input
                        id={`cline-model-id-${index}`}
                        value={row.id}
                        onChange={(e) =>
                          updateRow(index, { id: e.target.value })
                        }
                      />
                    </div>
                    <div className="space-y-2">
                      <Label htmlFor={`cline-model-name-${index}`}>
                        {t("cline.modelName")}
                      </Label>
                      <Input
                        id={`cline-model-name-${index}`}
                        value={row.model.name ?? ""}
                        onChange={(e) => {
                          const { name: _name, ...rest } = row.model;
                          updateRow(index, {
                            model: e.target.value
                              ? { ...rest, name: e.target.value }
                              : rest,
                          });
                        }}
                      />
                    </div>
                    {NUMBER_FIELDS.map(([field, label]) => (
                      <div key={field} className="space-y-2">
                        <Label htmlFor={`cline-model-${field}-${index}`}>
                          {t(label)}
                        </Label>
                        <Input
                          id={`cline-model-${field}-${index}`}
                          type="number"
                          min={0}
                          value={row.model[field] ?? ""}
                          onChange={(e) => {
                            const { [field]: _old, ...rest } = row.model;
                            updateRow(index, {
                              model:
                                e.target.value === ""
                                  ? rest
                                  : {
                                      ...rest,
                                      [field]: Number(e.target.value),
                                    },
                            });
                          }}
                        />
                      </div>
                    ))}
                  </div>
                </div>
              ))}
            </div>
          )}
        </fieldset>
        {!isOAuth && (
          <div className="space-y-2">
            <Label htmlFor="cline-settings-config">
              {t("provider.configJson")}
            </Label>
            <JsonEditor
              id="cline-settings-config"
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
                    return clineConfigSchema.safeParse(JSON.parse(text));
                  } catch {
                    return null;
                  }
                })();
                // The JSON editor cannot turn a custom provider into a built-in one.
                const valid =
                  parsed?.success === true &&
                  isClineCustomConfig(parsed.data) === isCustom;
                setJsonValid(valid);
                if (valid && parsed?.success) {
                  setConfig(parsed.data);
                  setRows(toRows(parsed.data));
                }
              }}
            />
          </div>
        )}
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
