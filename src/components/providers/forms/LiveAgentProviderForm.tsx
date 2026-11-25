import { useCallback, useEffect, useRef, useState } from "react";
import { useForm } from "react-hook-form";
import { useTranslation } from "react-i18next";
import { ChevronRight, Plus, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Form } from "@/components/ui/form";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Checkbox } from "@/components/ui/checkbox";
import { HoverTip } from "@/components/ui/hover-tip";
import { Switch } from "@/components/ui/switch";
import { toast } from "@/lib/toast";
import {
  fetchModelsForConfig,
  showFetchModelsError,
  type FetchedModel,
} from "@/lib/api/model-fetch";
import { useCommittableRef } from "@/hooks/useLatestRef";
import { useModelMetadataFill } from "@/hooks/useModelMetadataFill";
import { piPresetModelSources } from "@/config/presetModelMetadata";
import JsonEditor from "@/components/JsonEditor";
import { useDarkMode } from "@/hooks/useDarkMode";
import { useProvidersQuery } from "@/lib/query/queries";
import type { ProviderFormData } from "@/lib/schemas/provider";
import {
  LIVEAGENT_API_OPTIONS,
  liveagentProviderSchema,
  addLiveAgentModel,
  applyFetchedLiveAgentModels,
  changeLiveAgentModel,
  liveAgentFetchApiFormat,
  liveAgentModelEnabled,
  setLiveAgentModelEnabled,
  initialLiveAgentConfig,
  removeLiveAgentModel,
  type LiveAgentModel,
  type LiveAgentProviderConfig,
} from "@/lib/liveagentProvider";
import { BasicFormFields } from "./BasicFormFields";
import {
  FetchedModelPicker,
  type FetchedModelSelection,
} from "./FetchedModelPicker";
import {
  fillLiveAgentModel,
  metadataFilledAnything,
} from "./modelMetadataFill";
import { GptContextWindowHint, ModelDropdown } from "./shared";
import type { ProviderFormProps } from "./ProviderForm";

/** Radix Select items cannot use "" for "derive the protocol from `type`". */
const FROM_TYPE = "__from_type__";
/** Per-model API left empty: the model uses the provider's protocol. */
const INHERIT_API = "__inherit__";

type SamplingParams = NonNullable<LiveAgentModel["samplingParams"]>;

/** Empty input clears the parameter; no parameters left drops the object. */
function withSamplingParam(
  current: LiveAgentModel["samplingParams"],
  param: keyof SamplingParams,
  raw: string,
): LiveAgentModel["samplingParams"] {
  const next: SamplingParams = { ...(current ?? {}) };
  const value = raw.trim() === "" ? undefined : Number(raw);
  if (value === undefined || Number.isNaN(value)) delete next[param];
  else next[param] = value;
  return Object.keys(next).length > 0 ? next : undefined;
}

function apiHintKey(config: LiveAgentProviderConfig): string {
  switch (config.api) {
    case "openai-completions":
      return "completions";
    case "openai-responses":
      return "responses";
    case "anthropic-messages":
      return "anthropic";
    case "google-generative-ai":
    case "google-gemini":
      return "google";
    default:
      return "fromType";
  }
}

export function LiveAgentProviderForm({
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
  const dark = useDarkMode();
  const [providerKey, setProviderKey] = useState(
    providerId ?? crypto.randomUUID(),
  );
  const [config, setConfig] = useState<LiveAgentProviderConfig>(() =>
    initialLiveAgentConfig(initialData?.settingsConfig),
  );
  const [text, setText] = useState(JSON.stringify(config, null, 2));
  const [jsonValid, setJsonValid] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const { data: existing } = useProvidersQuery("liveagent");
  const form = useForm<ProviderFormData>({
    defaultValues: {
      name: initialData?.name ?? "",
      websiteUrl: initialData?.websiteUrl ?? "",
      notes: initialData?.notes ?? "",
      icon: initialData?.icon ?? "",
      iconColor: initialData?.iconColor ?? "",
      settingsConfig: text,
    },
  });
  const name = form.watch("name");
  const duplicate =
    !initialData &&
    Object.prototype.hasOwnProperty.call(
      existing?.providers ?? {},
      providerKey.trim(),
    );
  const ready = Boolean(
    jsonValid &&
      !duplicate &&
      providerKey.trim() &&
      name.trim() &&
      liveagentProviderSchema.safeParse(config).success &&
      (initialData ||
        (config.models.length &&
          config.baseUrl.trim() &&
          config.apiKey.trim())),
  );
  useEffect(() => {
    onSubmitReadyChange?.(ready);
  }, [ready, onSubmitReadyChange]);
  const update = (next: LiveAgentProviderConfig) => {
    setConfig(next);
    setText(JSON.stringify(next, null, 2));
    setJsonValid(true);
  };
  // Metadata fills can land after later edits: always apply to the latest config.
  const [configRef, commit] = useCommittableRef(config, update);

  // Stable row keys for expansion and late metadata fills (models have no ids
  // of their own while being typed).
  const keysRef = useRef<string[]>([]);
  while (keysRef.current.length < config.models.length) {
    keysRef.current.push(crypto.randomUUID());
  }
  keysRef.current.length = config.models.length;
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const toggleExpanded = (key: string) =>
    setExpanded((current) => {
      const next = new Set(current);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });

  const [fetchedModels, setFetchedModels] = useState<FetchedModel[]>([]);
  const [fetching, setFetching] = useState(false);
  const fetchGeneration = useRef(0);
  const fillMetadata = useModelMetadataFill({
    baseUrl: config.baseUrl,
    presets: piPresetModelSources,
    prefetch: fetchedModels.length > 0,
  });
  const fillModel = (key: string, id: string, silent?: boolean) => {
    if (!id.trim()) return;
    fillMetadata(
      id,
      (metadata) => {
        const at = keysRef.current.indexOf(key);
        const current = configRef.current.models[at];
        if (at < 0 || current?.id !== id) return false;
        const filled = fillLiveAgentModel(current, metadata);
        if (!metadataFilledAnything(current, filled)) return false;
        commit({
          ...configRef.current,
          models: configRef.current.models.map((model, i) =>
            i === at ? filled : model,
          ),
        });
        return true;
      },
      { silent },
    );
  };

  const handleFetchModels = useCallback(() => {
    const endpoint = config.baseUrl.trim();
    const requestHeaders = Object.fromEntries(
      (config.customHeaders ?? [])
        .filter((header) => header.key.trim())
        .map((header) => [header.key.trim(), header.value]),
    );
    const hasCredentials =
      Boolean(config.apiKey) || Object.keys(requestHeaders).length > 0;
    if (!endpoint || !hasCredentials) {
      showFetchModelsError(null, t, {
        hasApiKey: hasCredentials,
        hasBaseUrl: Boolean(endpoint),
      });
      return;
    }
    const userAgent = Object.entries(requestHeaders).find(
      ([key]) => key.toLowerCase() === "user-agent",
    )?.[1];
    const generation = ++fetchGeneration.current;
    setFetchedModels([]);
    setFetching(true);
    fetchModelsForConfig(
      endpoint,
      config.apiKey,
      config.isFullUrl ?? undefined,
      config.modelsUrl || undefined,
      userAgent,
      { apiFormat: liveAgentFetchApiFormat(config), requestHeaders },
    )
      .then((result) => {
        if (fetchGeneration.current !== generation) return;
        const models = [
          ...new Map(result.map((model) => [model.id, model])).values(),
        ];
        setFetchedModels(models);
        if (models.length === 0) toast.info(t("providerForm.fetchModelsEmpty"));
      })
      .catch((err) => {
        if (fetchGeneration.current !== generation) return;
        console.warn("[ModelFetch] Failed:", err);
        showFetchModelsError(err, t);
      })
      .finally(() => {
        if (fetchGeneration.current === generation) setFetching(false);
      });
  }, [config, t]);

  const handleApplyFetched = (selection: FetchedModelSelection) => {
    const current = configRef.current;
    const removed = new Set(selection.remove);
    const keptKeys = keysRef.current.filter(
      (_, i) => !removed.has(current.models[i]?.id),
    );
    const next = applyFetchedLiveAgentModels(current, selection);
    const addedKeys = next.models
      .slice(keptKeys.length)
      .map(() => crypto.randomUUID());
    keysRef.current = [...keptKeys, ...addedKeys];
    commit(next);
    // The picker already showed these values; fill without another toast.
    next.models
      .slice(keptKeys.length)
      .forEach((model, i) => fillModel(addedKeys[i], model.id, true));
  };

  const addModel = () => {
    keysRef.current.push(crypto.randomUUID());
    commit(addLiveAgentModel(configRef.current));
  };
  const removeModel = (index: number) => {
    const [key] = keysRef.current.splice(index, 1);
    setExpanded((current) => {
      const next = new Set(current);
      next.delete(key);
      return next;
    });
    commit(removeLiveAgentModel(configRef.current, index));
  };
  const changeModel = (
    index: number,
    patch: Parameters<typeof changeLiveAgentModel>[2],
  ) => commit(changeLiveAgentModel(configRef.current, index, patch));

  return (
    <Form {...form}>
      <form
        id="provider-form"
        className="space-y-5"
        onSubmit={form.handleSubmit(async (identity) => {
          if (!ready || busy) return;
          setBusy(true);
          onSubmittingChange?.(true);
          setError("");
          try {
            await onSubmit({
              ...identity,
              name: identity.name.trim(),
              providerKey: providerKey.trim(),
              meta: initialData?.meta,
              presetCategory: initialData?.category ?? "custom",
              settingsConfig: JSON.stringify({
                ...liveagentProviderSchema.parse(config),
                name: identity.name.trim(),
              }),
            });
          } catch (e) {
            setError(String(e));
          } finally {
            setBusy(false);
            onSubmittingChange?.(false);
          }
        })}
      >
        <p className="text-sm text-muted-foreground">
          {t("liveagent.restartRequired")}
        </p>
        {error && (
          <p role="alert" className="text-sm text-destructive">
            {error}
          </p>
        )}
        <fieldset
          disabled={busy || !jsonValid}
          className="min-w-0 space-y-5 disabled:opacity-50"
        >
          <BasicFormFields
            form={form}
            beforeNameSlot={
              <div className="space-y-2">
                <Label htmlFor="liveagent-key">
                  {t("liveagent.providerKey")}
                </Label>
                <Input
                  id="liveagent-key"
                  value={providerKey}
                  onChange={(e) => setProviderKey(e.target.value)}
                  disabled={Boolean(initialData)}
                  autoComplete="off"
                />
                {duplicate && (
                  <p role="alert" className="text-sm text-destructive">
                    {t("liveagent.duplicateKey")}
                  </p>
                )}
              </div>
            }
          />
          <div className="space-y-2">
            <Label htmlFor="liveagent-api">{t("liveagent.api")}</Label>
            <Select
              value={config.api || FROM_TYPE}
              onValueChange={(value) =>
                update({
                  ...config,
                  api:
                    value === FROM_TYPE
                      ? ""
                      : (value as LiveAgentProviderConfig["api"]),
                })
              }
            >
              <SelectTrigger id="liveagent-api" className="w-full">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {config.type && (
                  <SelectItem value={FROM_TYPE}>
                    {t("liveagent.apiFromType", { type: config.type })}
                  </SelectItem>
                )}
                {LIVEAGENT_API_OPTIONS.map((option) => (
                  <SelectItem key={option.value} value={option.value}>
                    {option.label}
                  </SelectItem>
                ))}
                {config.api === "google-gemini" && (
                  <SelectItem value="google-gemini">
                    Google Gemini (google-gemini)
                  </SelectItem>
                )}
              </SelectContent>
            </Select>
            <p className="text-xs text-fg-2">
              {t(`liveagent.apiHint.${apiHintKey(config)}`)}
            </p>
          </div>
          <div className="space-y-2">
            <Label htmlFor="liveagent-url">{t("liveagent.baseUrl")}</Label>
            <Input
              id="liveagent-url"
              value={config.baseUrl}
              onChange={(e) => update({ ...config, baseUrl: e.target.value })}
              placeholder="https://api.example.com/v1"
            />
          </div>
          <div className="space-y-2">
            <Label htmlFor="liveagent-key-secret">
              {t("liveagent.apiKey")}
            </Label>
            <Input
              id="liveagent-key-secret"
              type="password"
              autoComplete="new-password"
              value={config.apiKey}
              onChange={(e) => update({ ...config, apiKey: e.target.value })}
            />
          </div>
          <div className="space-y-3 border-l border-border pl-3">
            <div className="flex items-center justify-between gap-3">
              <Label>{t("liveagent.models")}</Label>
              <div className="flex gap-1">
                <FetchedModelPicker
                  models={fetchedModels}
                  configuredModelIds={config.models.map((model) => model.id)}
                  onApply={handleApplyFetched}
                  onFetch={handleFetchModels}
                  isFetching={fetching}
                  metadataSource={{
                    baseUrl: config.baseUrl,
                    presets: piPresetModelSources,
                  }}
                />
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  className="h-7 gap-1"
                  onClick={addModel}
                >
                  <Plus className="h-3.5 w-3.5" />
                  {t("liveagent.addModel")}
                </Button>
              </div>
            </div>
            {config.models.length === 0 ? (
              <p role="status" className="py-2 text-sm text-fg-2">
                {config.type
                  ? t("liveagent.noModelsTyped", { type: config.type })
                  : t("liveagent.noModels")}
              </p>
            ) : (
              <div className="space-y-2">
                <div className="flex items-center gap-2 px-1 text-xs text-fg-2">
                  <span className="w-9" />
                  <span className="w-5" />
                  <span className="flex-1">{t("liveagent.modelId")}</span>
                  <span className="flex-1">{t("liveagent.modelName")}</span>
                  <span className="w-9" />
                </div>
                {config.models.map((model, index) => {
                  const key = keysRef.current[index];
                  const isExpanded = expanded.has(key);
                  const label = model.id || String(index + 1);
                  return (
                    <div key={key} className="space-y-2">
                      <div className="flex items-center gap-2">
                        <Button
                          type="button"
                          variant="ghost"
                          size="icon"
                          className="h-9 w-9 shrink-0"
                          aria-expanded={isExpanded}
                          aria-label={t("liveagent.toggleModelDetails", {
                            model: label,
                          })}
                          onClick={() => toggleExpanded(key)}
                        >
                          <ChevronRight
                            className={`h-4 w-4 transition-transform motion-reduce:transition-none ${
                              isExpanded ? "rotate-90" : ""
                            }`}
                          />
                        </Button>
                        <HoverTip content={t("liveagent.enabled")}>
                          <Checkbox
                            className="w-5"
                            aria-label={t("liveagent.enableModel", {
                              model: label,
                            })}
                            checked={liveAgentModelEnabled(config, model.id)}
                            disabled={!model.id}
                            onCheckedChange={(checked) =>
                              commit(
                                setLiveAgentModelEnabled(
                                  configRef.current,
                                  model.id,
                                  checked === true,
                                ),
                              )
                            }
                          />
                        </HoverTip>
                        <div className="flex min-w-0 flex-1 gap-1">
                          <Input
                            id={`la-model-${key}`}
                            value={model.id}
                            placeholder="model-id"
                            aria-label={t("liveagent.modelId")}
                            className="min-w-0 flex-1"
                            onChange={(e) =>
                              changeModel(index, { id: e.target.value })
                            }
                            onBlur={(e) => fillModel(key, e.target.value)}
                          />
                          {fetchedModels.length > 0 && (
                            <ModelDropdown
                              models={fetchedModels}
                              onSelect={(id) => {
                                changeModel(index, { id });
                                fillModel(key, id);
                              }}
                            />
                          )}
                        </div>
                        <Input
                          id={`la-name-${key}`}
                          value={model.name ?? ""}
                          placeholder={model.id || undefined}
                          aria-label={t("liveagent.modelName")}
                          className="min-w-0 flex-1"
                          onChange={(e) =>
                            changeModel(index, {
                              name: e.target.value || undefined,
                            })
                          }
                        />
                        <HoverTip
                          content={t("liveagent.removeModel", { model: label })}
                        >
                          <Button
                            type="button"
                            variant="ghost"
                            size="icon"
                            className="h-9 w-9 shrink-0 text-fg-2 hover:text-destructive"
                            aria-label={t("liveagent.removeModel", {
                              model: label,
                            })}
                            onClick={() => removeModel(index)}
                          >
                            <Trash2 className="h-4 w-4" />
                          </Button>
                        </HoverTip>
                      </div>
                      {isExpanded && (
                        <div className="ml-9 grid gap-3 border-l-2 border-muted pl-4 sm:grid-cols-2">
                          <div className="flex min-h-9 flex-wrap items-center gap-x-8 gap-y-2 sm:col-span-2">
                            <div className="flex items-center gap-2.5">
                              <Label
                                htmlFor={`la-reasoning-${key}`}
                                className="cursor-pointer"
                              >
                                {t("liveagent.reasoning")}
                              </Label>
                              <Switch
                                id={`la-reasoning-${key}`}
                                checked={model.reasoning === true}
                                onCheckedChange={(checked) =>
                                  changeModel(index, { reasoning: checked })
                                }
                              />
                            </div>
                            <div className="flex items-center gap-2.5">
                              <Label
                                htmlFor={`la-image-${key}`}
                                className="cursor-pointer"
                              >
                                {t("liveagent.imageInput")}
                              </Label>
                              <Switch
                                id={`la-image-${key}`}
                                checked={Boolean(
                                  model.input?.includes("image"),
                                )}
                                onCheckedChange={(checked) =>
                                  changeModel(index, {
                                    input: checked
                                      ? Array.from(
                                          new Set([
                                            ...(model.input?.length
                                              ? model.input
                                              : ["text"]),
                                            "image",
                                          ]),
                                        )
                                      : (model.input ?? ["text"]).filter(
                                          (kind) => kind !== "image",
                                        ),
                                  })
                                }
                              />
                            </div>
                          </div>
                          {(["contextWindow", "maxTokens"] as const).map(
                            (field) => (
                              <div key={field} className="space-y-1">
                                <Label
                                  htmlFor={`la-${field}-${key}`}
                                  className="text-xs text-fg-2"
                                >
                                  {t(`liveagent.${field}`)}
                                </Label>
                                <Input
                                  id={`la-${field}-${key}`}
                                  type="number"
                                  min={0}
                                  step={1}
                                  placeholder={
                                    field === "contextWindow"
                                      ? "128000"
                                      : "32000"
                                  }
                                  value={model[field] ?? ""}
                                  onChange={(e) =>
                                    changeModel(index, {
                                      [field]: e.target.value
                                        ? Number(e.target.value)
                                        : undefined,
                                    })
                                  }
                                />
                                {field === "contextWindow" && (
                                  <GptContextWindowHint
                                    modelId={model.id}
                                    contextWindow={
                                      model.contextWindow ?? undefined
                                    }
                                  />
                                )}
                              </div>
                            ),
                          )}
                          <div className="space-y-1 sm:col-span-2">
                            <Label
                              htmlFor={`la-model-api-${key}`}
                              className="text-xs text-fg-2"
                            >
                              {t("liveagent.modelApi")}
                            </Label>
                            <Select
                              value={model.api || INHERIT_API}
                              onValueChange={(value) =>
                                changeModel(index, {
                                  api:
                                    value === INHERIT_API
                                      ? undefined
                                      : (value as LiveAgentModel["api"]),
                                })
                              }
                            >
                              <SelectTrigger
                                id={`la-model-api-${key}`}
                                className="w-full"
                              >
                                <SelectValue />
                              </SelectTrigger>
                              <SelectContent>
                                <SelectItem value={INHERIT_API}>
                                  {t("liveagent.modelApiInherit")}
                                </SelectItem>
                                {LIVEAGENT_API_OPTIONS.map((option) => (
                                  <SelectItem
                                    key={option.value}
                                    value={option.value}
                                  >
                                    {option.label}
                                  </SelectItem>
                                ))}
                              </SelectContent>
                            </Select>
                          </div>
                          {(["temperature", "top_p"] as const).map((param) => (
                            <div key={param} className="space-y-1">
                              <Label
                                htmlFor={`la-${param}-${key}`}
                                className="text-xs text-fg-2"
                              >
                                {t(`liveagent.sampling.${param}`)}
                              </Label>
                              <Input
                                id={`la-${param}-${key}`}
                                type="number"
                                min={0}
                                max={param === "top_p" ? 1 : 2}
                                step={0.05}
                                placeholder={t("liveagent.sampling.unset")}
                                value={model.samplingParams?.[param] ?? ""}
                                onChange={(e) =>
                                  changeModel(index, {
                                    samplingParams: withSamplingParam(
                                      model.samplingParams,
                                      param,
                                      e.target.value,
                                    ),
                                  })
                                }
                              />
                            </div>
                          ))}
                        </div>
                      )}
                    </div>
                  );
                })}
              </div>
            )}
          </div>
        </fieldset>
        <details className="space-y-2">
          <summary className="cursor-pointer text-sm">
            {t("liveagent.advanced")}
          </summary>
          <JsonEditor
            value={text}
            onChange={(value) => {
              setText(value);
              try {
                const parsed = liveagentProviderSchema.safeParse(
                  JSON.parse(value),
                );
                setJsonValid(parsed.success);
                if (parsed.success) setConfig(parsed.data);
              } catch {
                setJsonValid(false);
              }
            }}
            darkMode={dark}
          />
        </details>
        {!ready && (
          <p role="status" className="text-sm text-muted-foreground">
            {t("liveagent.validationHint")}
          </p>
        )}
        {showButtons && (
          <div className="flex justify-end gap-2">
            <Button type="button" variant="outline" onClick={onCancel}>
              {t("common.cancel")}
            </Button>
            <Button type="submit" disabled={!ready || busy}>
              {submitLabel ?? t("common.save")}
            </Button>
          </div>
        )}
      </form>
    </Form>
  );
}
