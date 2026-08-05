import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { ChevronRight, Download, Loader2, Plus, Trash2 } from "lucide-react";
import { useForm } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Form } from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import type { ProviderFormProps, ProviderFormValues } from "./ProviderForm";
import JsonEditor from "@/components/JsonEditor";
import { BasicFormFields } from "./BasicFormFields";
import { ProviderPresetSelector } from "./ProviderPresetSelector";
import { RequestHeadersEditor } from "./RequestHeadersEditor";
import { ApiKeySection, EndpointField, ModelDropdown } from "./shared";
import {
  findRequestHeaderValue,
  normalizeRequestHeaders,
} from "./helpers/requestHeaders";
import {
  piProviderPresets,
  type PiApiFormat,
  type PiProviderPreset,
} from "@/config/piProviderPresets";
import {
  fetchModelsForConfig,
  showFetchModelsError,
  type FetchedModel,
} from "@/lib/api/model-fetch";
import type { ModelsDevResponse } from "@/lib/modelsDevPricing";
import { loadModelsDevCatalog } from "@/lib/query/modelsDev";
import { useDarkMode } from "@/hooks/useDarkMode";
import { providerSchema, type ProviderFormData } from "@/lib/schemas/provider";
import type { ProviderCategory } from "@/types";
import { translatePiProviderMutationError } from "@/utils/errorUtils";
import {
  resolvePiModelMetadata,
  type PiModelMetadata,
} from "@/utils/piModelMetadata";

const PI_API_FORMATS = [
  { value: "openai-completions", label: "OpenAI Chat Completions" },
  { value: "openai-responses", label: "OpenAI Responses" },
  { value: "anthropic-messages", label: "Anthropic Messages" },
  { value: "google-generative-ai", label: "Google Generative AI" },
  { value: "bedrock-converse-stream", label: "Amazon Bedrock" },
] as const satisfies ReadonlyArray<{ value: PiApiFormat; label: string }>;

const ROOT_CONTROLLED_KEYS = new Set([
  "name",
  "baseUrl",
  "api",
  "apiKey",
  "headers",
  "models",
]);
const MODEL_CONTROLLED_KEYS = new Set([
  "id",
  "name",
  "reasoning",
  "input",
  "contextWindow",
  "maxTokens",
]);

interface PiModelDraft {
  key: string;
  id: string;
  name: string;
  reasoning: unknown;
  input: unknown;
  contextWindow: string;
  maxTokens: string;
  passthrough: Record<string, unknown>;
  preferredMetadataProvider: string | null;
  autoMetadata: PiModelMetadata | null;
  persistAutoMetadata: boolean;
  overrides: PiModelOverrides;
}

type PiModelOverrideField =
  | "name"
  | "reasoning"
  | "imageInput"
  | "contextWindow"
  | "maxTokens";

type PiModelOverrides = Record<PiModelOverrideField, boolean>;

function emptyModelOverrides(): PiModelOverrides {
  return {
    name: false,
    reasoning: false,
    imageInput: false,
    contextWindow: false,
    maxTokens: false,
  };
}

class PiFormValidationError extends Error {
  constructor(
    message: string,
    readonly fieldSelector?: string,
    readonly revealAdvanced = false,
  ) {
    super(message);
    this.name = "PiFormValidationError";
  }
}

function validatePiField<T>(
  operation: () => T,
  fieldSelector: string,
  revealAdvanced = false,
): T {
  try {
    return operation();
  } catch (error) {
    throw new PiFormValidationError(
      error instanceof Error ? error.message : String(error),
      fieldSelector,
      revealAdvanced,
    );
  }
}

function objectWithout(
  value: Record<string, unknown>,
  denied: Set<string>,
): Record<string, unknown> {
  return Object.fromEntries(
    Object.entries(value).filter(([key]) => !denied.has(key)),
  );
}

function asObject(value: unknown): Record<string, unknown> {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : {};
}

function optionalText(value: unknown): string {
  return typeof value === "string" ? value : "";
}

function optionalNumberText(value: unknown): string {
  return typeof value === "number" && Number.isFinite(value)
    ? String(value)
    : "";
}

function hasOwn(value: Record<string, unknown>, key: string): boolean {
  return Object.prototype.hasOwnProperty.call(value, key);
}

function stringRecord(value: Record<string, unknown>): Record<string, string> {
  return Object.fromEntries(
    Object.entries(value).filter(
      (entry): entry is [string, string] => typeof entry[1] === "string",
    ),
  );
}

function validateAbsoluteHttpUrl(value: string, errorMessage: string): void {
  let parsed: URL;
  try {
    parsed = new URL(value);
  } catch {
    throw new Error(errorMessage);
  }
  if (parsed.protocol !== "http:" && parsed.protocol !== "https:") {
    throw new Error(errorMessage);
  }
}

function optionalPositiveNumber(
  value: string,
  errorMessage: string,
  fieldSelector: string,
): number | undefined {
  if (value.trim() === "") return undefined;
  const parsed = Number(value);
  if (!Number.isFinite(parsed) || parsed <= 0) {
    throw new PiFormValidationError(errorMessage, fieldSelector, true);
  }
  return parsed;
}

function supportsImageInput(value: unknown): boolean {
  return Array.isArray(value) && value.includes("image");
}

function withImageInput(value: unknown, enabled: boolean): string[] {
  const additionalInputTypes = Array.isArray(value)
    ? value.filter(
        (item): item is string =>
          typeof item === "string" && item !== "text" && item !== "image",
      )
    : [];
  return [
    "text",
    ...(enabled ? ["image"] : []),
    ...new Set(additionalInputTypes),
  ];
}

function applyModelAutofill(
  model: PiModelDraft,
  metadata?: PiModelMetadata,
): PiModelDraft {
  return {
    ...model,
    name: model.overrides.name ? model.name : (metadata?.name ?? model.id),
    reasoning: model.overrides.reasoning
      ? model.reasoning
      : metadata?.reasoning,
    input: model.overrides.imageInput
      ? model.input
      : metadata?.imageInput === undefined
        ? undefined
        : withImageInput(undefined, metadata.imageInput),
    contextWindow: model.overrides.contextWindow
      ? model.contextWindow
      : optionalNumberText(metadata?.contextWindow),
    maxTokens: model.overrides.maxTokens
      ? model.maxTokens
      : optionalNumberText(metadata?.maxTokens),
    autoMetadata: metadata ?? null,
  };
}

function modelDraft(
  value: unknown,
  options: { auto?: boolean; metadata?: PiModelMetadata } = {},
): PiModelDraft {
  const model = asObject(value);
  const draft: PiModelDraft = {
    key: crypto.randomUUID(),
    id: optionalText(model.id),
    name: optionalText(model.name),
    reasoning: model.reasoning,
    input: model.input,
    contextWindow: optionalNumberText(model.contextWindow),
    maxTokens: optionalNumberText(model.maxTokens),
    passthrough: objectWithout(model, MODEL_CONTROLLED_KEYS),
    preferredMetadataProvider: null,
    autoMetadata: options.metadata ?? null,
    persistAutoMetadata: options.auto ?? false,
    overrides: options.auto
      ? emptyModelOverrides()
      : {
          name: hasOwn(model, "name"),
          reasoning: hasOwn(model, "reasoning"),
          imageInput: hasOwn(model, "input"),
          contextWindow: hasOwn(model, "contextWindow"),
          maxTokens: hasOwn(model, "maxTokens"),
        },
  };
  return options.auto ? applyModelAutofill(draft, options.metadata) : draft;
}

function newModel(): PiModelDraft {
  return {
    key: crypto.randomUUID(),
    id: "",
    name: "",
    reasoning: undefined,
    input: undefined,
    contextWindow: "",
    maxTokens: "",
    // Unknown models stay id-only until an exact preset/catalog match or a
    // manual override supplies capability metadata.
    passthrough: {},
    preferredMetadataProvider: null,
    autoMetadata: null,
    persistAutoMetadata: false,
    overrides: emptyModelOverrides(),
  };
}

function hasAnyModelOverrides(model: PiModelDraft): boolean {
  return Object.values(model.overrides).some(Boolean);
}

function shouldPersistModelField(
  model: PiModelDraft,
  field: PiModelOverrideField,
): boolean {
  return model.overrides[field] || model.persistAutoMetadata;
}

function modelMetadataStatusKey(
  model: PiModelDraft,
  isLoading: boolean,
  lookupComplete: boolean,
): string {
  const hasOverrides = hasAnyModelOverrides(model);
  if (model.autoMetadata && hasOverrides) {
    return "pi.form.modelMetadataOverridden";
  }
  if (model.autoMetadata) return "pi.form.modelMetadataAutofilled";
  if (hasOverrides) return "pi.form.modelMetadataManual";
  if (model.id && isLoading) return "pi.form.modelMetadataLoading";
  if (model.id && lookupComplete) return "pi.form.modelMetadataUnknown";
  return "pi.form.modelMetadataHint";
}

function modelPreview(model: PiModelDraft): Record<string, unknown> {
  const displayName = model.name.trim();
  const previewNumber = (value: string): number | string | undefined => {
    if (!value.trim()) return undefined;
    const parsed = Number(value);
    return Number.isFinite(parsed) && parsed > 0 ? parsed : value;
  };
  const contextWindow = previewNumber(model.contextWindow);
  const maxTokens = previewNumber(model.maxTokens);

  return {
    ...model.passthrough,
    id: model.id,
    ...(displayName &&
    (model.overrides.name ||
      (model.persistAutoMetadata && displayName !== model.id))
      ? { name: displayName }
      : {}),
    ...(model.reasoning !== undefined &&
    shouldPersistModelField(model, "reasoning")
      ? { reasoning: model.reasoning }
      : {}),
    ...(model.input !== undefined &&
    shouldPersistModelField(model, "imageInput")
      ? { input: model.input }
      : {}),
    ...(contextWindow !== undefined &&
    shouldPersistModelField(model, "contextWindow")
      ? { contextWindow }
      : {}),
    ...(maxTokens !== undefined && shouldPersistModelField(model, "maxTokens")
      ? { maxTokens }
      : {}),
  };
}

function buildPiSettingsConfig({
  passthrough,
  nativeName,
  baseUrl,
  api,
  includeApi,
  apiKey,
  headers,
  models,
}: {
  passthrough: Record<string, unknown>;
  nativeName?: string;
  baseUrl: string;
  api: string;
  includeApi: boolean;
  apiKey: string;
  headers: Record<string, string>;
  models: Record<string, unknown>[];
}): Record<string, unknown> {
  return {
    ...passthrough,
    ...(nativeName !== undefined ? { name: nativeName } : {}),
    ...(baseUrl.trim() ? { baseUrl: baseUrl.trim() } : {}),
    ...(includeApi && api.trim() ? { api: api.trim() } : {}),
    ...(apiKey ? { apiKey } : {}),
    ...(Object.keys(headers).length > 0 ? { headers } : {}),
    models,
  };
}

export function PiProviderForm({
  providerId,
  submitLabel,
  onSubmit,
  onCancel,
  onSubmittingChange,
  onSubmitReadyChange,
  initialData,
  showButtons = true,
}: ProviderFormProps) {
  const { t } = useTranslation();
  const isDarkMode = useDarkMode();
  const initialConfig = useMemo(
    () => asObject(initialData?.settingsConfig),
    [initialData?.settingsConfig],
  );
  const isEdit = Boolean(initialData);
  const initialNativeName = optionalText(initialConfig.name);
  const initialDisplayName = initialData?.name ?? initialNativeName;
  const initialConfigHasNativeName = hasOwn(initialConfig, "name");
  const resolveNativeName = useCallback(
    (displayName: string): string | undefined => {
      const nextName = displayName.trim();
      if (!isEdit || nextName !== initialDisplayName.trim()) {
        return nextName;
      }
      return initialConfigHasNativeName ? initialNativeName : undefined;
    },
    [initialConfigHasNativeName, initialDisplayName, initialNativeName, isEdit],
  );
  const [selectedPresetId, setSelectedPresetId] = useState<string | null>(
    isEdit ? null : "custom",
  );
  const [selectedPreset, setSelectedPreset] = useState<PiProviderPreset | null>(
    null,
  );
  const [category, setCategory] = useState<ProviderCategory>(
    initialData?.category ?? "custom",
  );
  const [providerKey, setProviderKey] = useState(providerId ?? "");
  const [baseUrl, setBaseUrl] = useState(optionalText(initialConfig.baseUrl));
  const [api, setApi] = useState(
    () => optionalText(initialConfig.api) || "openai-completions",
  );
  const [includeApi, setIncludeApi] = useState(
    () => !isEdit || hasOwn(initialConfig, "api"),
  );
  const [apiKey, setApiKey] = useState(optionalText(initialConfig.apiKey));
  const initialHeaders = useMemo(
    () => asObject(initialConfig.headers),
    [initialConfig.headers],
  );
  const [providerHeaders, setProviderHeaders] = useState<
    Record<string, string>
  >(() => stringRecord(initialHeaders));
  const [providerPassthrough, setProviderPassthrough] = useState<
    Record<string, unknown>
  >(() => objectWithout(initialConfig, ROOT_CONTROLLED_KEYS));
  const [fetchedModels, setFetchedModels] = useState<FetchedModel[]>([]);
  const [isFetchingModels, setIsFetchingModels] = useState(false);
  const modelsDevCatalogRef = useRef<ModelsDevResponse | null>(null);
  const [isModelMetadataLoading, setIsModelMetadataLoading] = useState(false);
  const [modelMetadataLookupComplete, setModelMetadataLookupComplete] =
    useState(false);
  const [formError, setFormError] = useState<string | null>(null);
  const initialModels = useMemo<PiModelDraft[]>(() => {
    const configured = Array.isArray(initialConfig.models)
      ? initialConfig.models
      : [];
    return configured.map((model) => modelDraft(model));
  }, [initialConfig.models]);
  const [models, setModels] = useState<PiModelDraft[]>(initialModels);
  const [expandedModelKeys, setExpandedModelKeys] = useState<Set<string>>(
    () => new Set(),
  );
  const identityDefaults = useMemo<ProviderFormData>(
    () => ({
      name: initialData?.name ?? optionalText(initialConfig.name),
      websiteUrl: initialData?.websiteUrl ?? "",
      notes: initialData?.notes ?? "",
      settingsConfig: "{}",
      icon: initialData?.icon ?? "",
      iconColor: initialData?.iconColor ?? "",
    }),
    [initialConfig, initialData],
  );
  const form = useForm<ProviderFormData>({
    resolver: zodResolver(providerSchema),
    defaultValues: identityDefaults,
    mode: "onSubmit",
  });
  const hasConfigurationSelection = isEdit || selectedPresetId !== null;
  const isSubmitReady = hasConfigurationSelection;

  useEffect(() => {
    onSubmitReadyChange?.(isSubmitReady);
  }, [isSubmitReady, onSubmitReadyChange]);

  const presetEntries = useMemo(
    () =>
      piProviderPresets.map((preset, index) => ({
        id: `pi-${index}`,
        preset,
      })),
    [],
  );

  const selectPreset = (id: string) => {
    setFormError(null);
    setSelectedPresetId(id);
    if (id === "custom") {
      setSelectedPreset(null);
      setCategory("custom");
      setProviderKey("");
      form.reset(identityDefaults);
      setBaseUrl("");
      setApi("openai-completions");
      setIncludeApi(true);
      setApiKey("");
      setProviderHeaders({});
      setProviderPassthrough({});
      setFetchedModels([]);
      setModels([]);
      setExpandedModelKeys(new Set());
      return;
    }
    const entry = presetEntries.find((candidate) => candidate.id === id);
    if (!entry) return;
    const preset = entry.preset;
    setSelectedPreset(preset);
    setCategory(preset.category ?? "custom");
    setProviderKey(preset.providerKey);
    form.reset({
      name: preset.settingsConfig.name,
      websiteUrl: preset.websiteUrl,
      notes: "",
      settingsConfig: "{}",
      icon: preset.icon ?? "",
      iconColor: preset.iconColor ?? "",
    });
    setBaseUrl(preset.settingsConfig.baseUrl);
    setApi(preset.settingsConfig.api);
    setIncludeApi(true);
    setApiKey("");
    const presetConfig = asObject(preset.settingsConfig);
    setProviderHeaders(stringRecord(asObject(presetConfig.headers)));
    setProviderPassthrough(objectWithout(presetConfig, ROOT_CONTROLLED_KEYS));
    setFetchedModels([]);
    const nextModels = preset.settingsConfig.models.map((model) =>
      modelDraft(model, {
        auto: true,
        metadata: resolvePiModelMetadata(model.id, {
          selectedPreset: preset,
          modelsDevCatalog: modelsDevCatalogRef.current,
        }),
      }),
    );
    setModels(nextModels);
    setExpandedModelKeys(new Set());
  };

  const resolveMetadataForModel = useCallback(
    (
      model: Pick<PiModelDraft, "id" | "preferredMetadataProvider">,
      catalog: ModelsDevResponse | null = modelsDevCatalogRef.current,
    ) =>
      resolvePiModelMetadata(model.id, {
        selectedPreset,
        modelsDevCatalog: catalog,
        preferredProvider: model.preferredMetadataProvider,
      }),
    [selectedPreset],
  );

  const updateModelOverride = (
    key: string,
    field: PiModelOverrideField,
    update: Partial<Omit<PiModelDraft, "key" | "overrides">>,
  ) => {
    setModels((current) =>
      current.map((model) =>
        model.key === key
          ? {
              ...model,
              ...update,
              overrides: { ...model.overrides, [field]: true },
            }
          : model,
      ),
    );
  };

  const changeModelId = (
    key: string,
    id: string,
    options: {
      preferredProvider?: string | null;
      resetOverrides?: boolean;
    } = {},
  ) => {
    setModels((current) =>
      current.map((model) => {
        if (model.key !== key) return model;
        const nextModel: PiModelDraft = {
          ...model,
          id,
          persistAutoMetadata: model.persistAutoMetadata || id !== model.id,
          preferredMetadataProvider:
            "preferredProvider" in options
              ? (options.preferredProvider ?? null)
              : model.preferredMetadataProvider,
          overrides: options.resetOverrides
            ? emptyModelOverrides()
            : model.overrides,
        };
        return applyModelAutofill(
          nextModel,
          resolveMetadataForModel(nextModel),
        );
      }),
    );
  };

  const restoreModelAutofill = (key: string) => {
    setModels((current) =>
      current.map((model) => {
        if (model.key !== key) return model;
        const nextModel = {
          ...model,
          persistAutoMetadata: true,
          overrides: emptyModelOverrides(),
        };
        return applyModelAutofill(
          nextModel,
          resolveMetadataForModel(nextModel),
        );
      }),
    );
  };

  const addModel = () => {
    const model = newModel();
    setModels((current) => [...current, model]);
  };

  const removeModel = (key: string) => {
    const nextModels = models.filter((model) => model.key !== key);
    setModels(nextModels);
    setExpandedModelKeys((current) => {
      const next = new Set(current);
      next.delete(key);
      return next;
    });
  };

  const toggleModelDetails = (key: string) => {
    setExpandedModelKeys((current) => {
      const next = new Set(current);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  };

  const hasModelId = models.some((model) => model.id.length > 0);
  useEffect(() => {
    if (!hasModelId) return;
    let cancelled = false;
    setIsModelMetadataLoading(true);
    void loadModelsDevCatalog()
      .then((catalog) => {
        modelsDevCatalogRef.current = catalog;
        if (cancelled) return;
        setModels((current) =>
          current.map((model) =>
            applyModelAutofill(
              model,
              resolvePiModelMetadata(model.id, {
                selectedPreset,
                modelsDevCatalog: catalog,
                preferredProvider: model.preferredMetadataProvider,
              }),
            ),
          ),
        );
        setModelMetadataLookupComplete(true);
      })
      .catch(() => {
        if (!cancelled) setModelMetadataLookupComplete(true);
      })
      .finally(() => {
        if (!cancelled) setIsModelMetadataLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [hasModelId, selectedPreset]);

  const handleFetchModels = useCallback(() => {
    const endpoint = baseUrl.trim();
    const requestHeaders = normalizeRequestHeaders(providerHeaders);
    const hasCredentials =
      Boolean(apiKey) || Object.keys(requestHeaders).length > 0;
    if (!endpoint || !hasCredentials) {
      showFetchModelsError(null, t, {
        hasApiKey: hasCredentials,
        hasBaseUrl: Boolean(endpoint),
      });
      return;
    }

    const customUserAgent = findRequestHeaderValue(
      requestHeaders,
      "user-agent",
    );

    // Warm the shared public catalog while the upstream model list loads.
    // This enrichment is optional and must not affect the fetch result.
    void loadModelsDevCatalog()
      .then((catalog) => {
        modelsDevCatalogRef.current = catalog;
        setModels((current) =>
          current.map((model) =>
            applyModelAutofill(
              model,
              resolvePiModelMetadata(model.id, {
                selectedPreset,
                modelsDevCatalog: catalog,
                preferredProvider: model.preferredMetadataProvider,
              }),
            ),
          ),
        );
        setModelMetadataLookupComplete(true);
      })
      .catch(() => undefined);

    setIsFetchingModels(true);
    fetchModelsForConfig(
      endpoint,
      apiKey,
      undefined,
      undefined,
      customUserAgent,
      {
        apiFormat: api,
        requestHeaders,
      },
    )
      .then((result) => {
        setFetchedModels(result);
        if (result.length === 0) {
          toast.info(t("providerForm.fetchModelsEmpty"));
        } else {
          toast.success(
            t("providerForm.fetchModelsSuccess", { count: result.length }),
          );
        }
      })
      .catch((error) => {
        console.warn("[ModelFetch] Failed:", error);
        showFetchModelsError(error, t);
      })
      .finally(() => setIsFetchingModels(false));
  }, [api, apiKey, baseUrl, providerHeaders, selectedPreset, t]);

  const submit = async (identity: ProviderFormData) => {
    onSubmittingChange?.(true);
    setFormError(null);
    try {
      if (!isEdit && selectedPresetId === null) {
        throw new PiFormValidationError(t("pi.form.selectPresetRequired"));
      }
      const trimmedName = identity.name.trim();
      const trimmedKey = providerKey.trim();
      if (!trimmedName) {
        throw new PiFormValidationError(
          t("pi.form.nameRequired"),
          'input[name="name"]',
        );
      }
      if (!isEdit && !trimmedKey) {
        throw new PiFormValidationError(
          t("pi.form.providerKeyRequired"),
          "#pi-provider-key",
        );
      }
      if (selectedPreset && apiKey.length === 0) {
        throw new PiFormValidationError(
          t("pi.form.credentialRequired"),
          "#pi-api-key",
        );
      }
      if (models.length === 0) {
        throw new PiFormValidationError(
          t("pi.form.modelRequired"),
          "#pi-add-model",
          true,
        );
      }

      const headers = normalizeRequestHeaders(providerHeaders);
      const seen = new Set<string>();
      const normalizedModels = models.map((model, index) => {
        // Pinned Pi treats model IDs as opaque, exact strings. In particular,
        // its schema accepts whitespace-only and edge-whitespace IDs; trimming
        // here would silently rename an imported model.
        const id = model.id;
        if (id.length === 0) {
          throw new PiFormValidationError(
            t("pi.form.modelIdRequired", { index: index + 1 }),
            `#pi-model-id-${model.key}`,
            true,
          );
        }
        if (seen.has(id)) {
          throw new PiFormValidationError(
            t("pi.form.duplicateModel", { id }),
            `#pi-model-id-${model.key}`,
            true,
          );
        }
        seen.add(id);
        const contextWindow = optionalPositiveNumber(
          model.contextWindow,
          t("pi.form.positiveNumberRequired", {
            label: t("pi.form.contextWindow"),
          }),
          `#pi-model-context-window-${model.key}`,
        );
        const maxTokens = optionalPositiveNumber(
          model.maxTokens,
          t("pi.form.positiveNumberRequired", {
            label: t("pi.form.maxTokens"),
          }),
          `#pi-model-max-tokens-${model.key}`,
        );
        // Pi's schema supports rare per-model api/baseUrl overrides. Keep
        // imported values losslessly, but use the provider-level format and
        // endpoint as the normal product model.
        const modelApi =
          typeof model.passthrough.api === "string"
            ? model.passthrough.api.trim()
            : "";
        const modelBaseUrl =
          typeof model.passthrough.baseUrl === "string"
            ? model.passthrough.baseUrl.trim()
            : "";
        if (!modelApi && !api.trim()) {
          throw new PiFormValidationError(
            t("pi.form.effectiveApiRequired", { id }),
            "#pi-provider-api-select",
            true,
          );
        }
        const effectiveUrl = modelBaseUrl || baseUrl.trim();
        if (!effectiveUrl) {
          throw new PiFormValidationError(
            t("pi.form.effectiveBaseUrlRequired", { id }),
            "#pi-provider-base-url",
          );
        }
        const displayName = model.name.trim();
        return {
          ...model.passthrough,
          id,
          ...(displayName &&
          (model.overrides.name ||
            (model.persistAutoMetadata && displayName !== model.id))
            ? { name: displayName }
            : {}),
          ...(model.reasoning !== undefined &&
          shouldPersistModelField(model, "reasoning")
            ? { reasoning: model.reasoning }
            : {}),
          ...(model.input !== undefined &&
          shouldPersistModelField(model, "imageInput")
            ? { input: model.input }
            : {}),
          ...(contextWindow !== undefined &&
          shouldPersistModelField(model, "contextWindow")
            ? { contextWindow }
            : {}),
          ...(maxTokens !== undefined &&
          shouldPersistModelField(model, "maxTokens")
            ? { maxTokens }
            : {}),
        };
      });
      if (baseUrl.trim()) {
        validatePiField(
          () =>
            validateAbsoluteHttpUrl(
              baseUrl.trim(),
              t("pi.form.absoluteHttpUrlRequired", {
                label: t("providerForm.apiEndpoint"),
              }),
            ),
          "#pi-provider-base-url",
          true,
        );
      }

      const settingsConfig = buildPiSettingsConfig({
        passthrough: providerPassthrough,
        nativeName: resolveNativeName(trimmedName),
        baseUrl,
        api,
        includeApi,
        apiKey,
        headers,
        models: normalizedModels,
      });
      const values: ProviderFormValues = {
        name: trimmedName,
        websiteUrl: identity.websiteUrl?.trim() ?? "",
        notes: identity.notes?.trim() ?? "",
        settingsConfig: JSON.stringify(settingsConfig),
        icon: identity.icon || selectedPreset?.icon || "pi",
        iconColor: identity.iconColor || selectedPreset?.iconColor || "",
        providerKey: isEdit ? providerId : trimmedKey,
        presetId: selectedPresetId ?? undefined,
        presetCategory: category,
        meta: initialData?.meta,
        ...(isEdit ? { expectedSettingsConfig: initialConfig } : {}),
      };
      await onSubmit(values);
    } catch (error) {
      const rawMessage = error instanceof Error ? error.message : String(error);
      const message =
        error instanceof PiFormValidationError
          ? rawMessage
          : translatePiProviderMutationError(rawMessage, t) || rawMessage;
      setFormError(message);
      if (error instanceof PiFormValidationError) {
        const modelDetailsMatch = error.fieldSelector?.match(
          /^#pi-model-(?:context-window|max-tokens)-(.+)$/,
        );
        if (modelDetailsMatch) {
          setExpandedModelKeys((current) => {
            const next = new Set(current);
            next.add(modelDetailsMatch[1]);
            return next;
          });
        }
        if (error.fieldSelector) {
          requestAnimationFrame(() => {
            document.querySelector<HTMLElement>(error.fieldSelector!)?.focus();
          });
        }
        toast.error(message);
      }
    } finally {
      onSubmittingChange?.(false);
    }
  };

  const presetCategoryLabels = useMemo<Record<string, string>>(
    () => ({
      official: t("providerForm.categoryOfficial"),
      cn_official: t("providerForm.categoryCnOfficial"),
      aggregator: t("providerForm.categoryAggregation"),
      third_party: t("providerForm.categoryThirdParty"),
      custom: t("providerPreset.custom"),
    }),
    [t],
  );
  const isKnownApiFormat = PI_API_FORMATS.some(
    (format) => format.value === api,
  );
  const previewName = form.watch("name");
  const settingsConfigPreview = useMemo(
    () =>
      JSON.stringify(
        buildPiSettingsConfig({
          passthrough: providerPassthrough,
          nativeName: resolveNativeName(previewName),
          baseUrl,
          api,
          includeApi,
          apiKey,
          headers: normalizeRequestHeaders(providerHeaders),
          models: models.map(modelPreview),
        }),
        null,
        2,
      ),
    [
      api,
      apiKey,
      baseUrl,
      includeApi,
      models,
      previewName,
      providerHeaders,
      providerPassthrough,
      resolveNativeName,
    ],
  );

  return (
    <Form {...form}>
      <form
        id="provider-form"
        onSubmit={form.handleSubmit(submit)}
        onChangeCapture={() => {
          if (formError) setFormError(null);
        }}
        className="space-y-6 glass rounded-xl p-6 border border-white/10"
      >
        {!isEdit && (
          <ProviderPresetSelector
            selectedPresetId={selectedPresetId}
            presetEntries={presetEntries}
            presetCategoryLabels={presetCategoryLabels}
            onPresetChange={selectPreset}
            category={category}
          />
        )}

        {formError && (
          <div
            role="alert"
            aria-live="assertive"
            className="rounded-lg border border-destructive/30 bg-destructive/10 px-4 py-3 text-sm text-destructive"
          >
            {formError}
          </div>
        )}

        {hasConfigurationSelection && (
          <>
            <BasicFormFields
              form={form}
              beforeNameSlot={
                isEdit || selectedPresetId === "custom" ? (
                  <div className="space-y-2">
                    <Label htmlFor="pi-provider-key">
                      {t("pi.form.providerKey")}
                      <span className="text-destructive ml-1">*</span>
                    </Label>
                    <Input
                      id="pi-provider-key"
                      value={providerKey}
                      onChange={(event) =>
                        setProviderKey(
                          event.target.value
                            .toLowerCase()
                            .replace(/[^a-z0-9-]/g, ""),
                        )
                      }
                      disabled={isEdit}
                      placeholder="my-provider"
                      autoComplete="off"
                    />
                    <p className="text-xs text-muted-foreground">
                      {t("pi.form.providerKeyHint")}
                    </p>
                  </div>
                ) : undefined
              }
            />

            <ApiKeySection
              id="pi-api-key"
              label={t("pi.form.credential")}
              value={apiKey}
              onChange={setApiKey}
              category={category}
              shouldShowLink={Boolean(selectedPreset?.apiKeyUrl)}
              websiteUrl={selectedPreset?.apiKeyUrl ?? ""}
              isPartner={selectedPreset?.isPartner}
              partnerPromotionKey={selectedPreset?.partnerPromotionKey}
              placeholder={{
                official: t("pi.form.apiKeyPlaceholder"),
                thirdParty: t("pi.form.apiKeyPlaceholder"),
              }}
            />

            <EndpointField
              id="pi-provider-base-url"
              label={t("providerForm.apiEndpoint")}
              value={baseUrl}
              onChange={setBaseUrl}
              placeholder="https://api.example.com/v1"
            />

            <Field
              label={t("pi.form.apiFormat", { defaultValue: "接口格式" })}
              htmlFor="pi-provider-api-select"
            >
              <Select
                value={api}
                onValueChange={(value) => {
                  setApi(value);
                  setIncludeApi(true);
                }}
              >
                <SelectTrigger id="pi-provider-api-select" className="w-full">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {PI_API_FORMATS.map((format) => (
                    <SelectItem key={format.value} value={format.value}>
                      {format.label}
                    </SelectItem>
                  ))}
                  {!isKnownApiFormat && api && (
                    <SelectItem value={api}>{api}</SelectItem>
                  )}
                </SelectContent>
              </Select>
              <p className="text-xs text-muted-foreground">
                {t("pi.form.apiFormatHint", {
                  defaultValue:
                    "默认使用 OpenAI Chat Completions；预设通常无需修改。",
                })}
              </p>
            </Field>

            <div
              id="pi-models-section"
              tabIndex={-1}
              className="space-y-3 outline-none"
            >
              <div className="flex items-center justify-between gap-3">
                <h3 className="text-sm font-normal leading-5">
                  {t("pi.form.models", { defaultValue: "模型配置" })}
                </h3>
                <div className="flex gap-1">
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    onClick={handleFetchModels}
                    disabled={isFetchingModels}
                    className="h-7 gap-1"
                  >
                    {isFetchingModels ? (
                      <Loader2 className="h-3.5 w-3.5 animate-spin" />
                    ) : (
                      <Download className="h-3.5 w-3.5" />
                    )}
                    {t("providerForm.fetchModels")}
                  </Button>
                  <Button
                    id="pi-add-model"
                    type="button"
                    variant="outline"
                    size="sm"
                    onClick={addModel}
                    className="h-7 gap-1"
                  >
                    <Plus className="h-3.5 w-3.5" />
                    {t("pi.form.addModel")}
                  </Button>
                </div>
              </div>

              {models.length === 0 ? (
                <p role="status" className="py-2 text-sm text-muted-foreground">
                  {t("pi.form.noModels", {
                    defaultValue: "暂无模型配置",
                  })}
                </p>
              ) : (
                <div className="space-y-2">
                  <div className="flex items-center gap-2 px-1 text-xs text-muted-foreground">
                    <span className="w-9" />
                    <span className="flex-1">{t("pi.form.modelId")}</span>
                    <span className="flex-1">{t("pi.form.modelName")}</span>
                    <span className="w-9" />
                  </div>
                  {models.map((model) => {
                    const metadataStatusId = `pi-model-metadata-status-${model.key}`;
                    const canRestoreAutofill =
                      Boolean(model.autoMetadata) &&
                      hasAnyModelOverrides(model);
                    return (
                      <div key={model.key} className="space-y-2">
                        <div className="flex items-center gap-2">
                          <Button
                            type="button"
                            variant="ghost"
                            size="icon"
                            onClick={() => toggleModelDetails(model.key)}
                            aria-label={t("pi.form.toggleModelDetails", {
                              defaultValue: "展开或收起模型详情",
                            })}
                            className="h-9 w-9 shrink-0"
                          >
                            <ChevronRight
                              className={`h-4 w-4 transition-transform motion-reduce:transition-none ${
                                expandedModelKeys.has(model.key)
                                  ? "rotate-90"
                                  : ""
                              }`}
                            />
                          </Button>
                          <div className="flex min-w-0 flex-1 gap-1">
                            <Input
                              id={`pi-model-id-${model.key}`}
                              value={model.id}
                              onChange={(event) =>
                                changeModelId(model.key, event.target.value)
                              }
                              placeholder="model-id"
                              aria-label={t("pi.form.modelId")}
                              className="min-w-0 flex-1"
                            />
                            {fetchedModels.length > 0 && (
                              <ModelDropdown
                                models={fetchedModels}
                                onSelect={(id) => {
                                  const fetchedModel = fetchedModels.find(
                                    (candidate) => candidate.id === id,
                                  );
                                  changeModelId(model.key, id, {
                                    preferredProvider:
                                      fetchedModel?.ownedBy ?? null,
                                    resetOverrides: true,
                                  });
                                }}
                              />
                            )}
                          </div>
                          <Input
                            id={`pi-model-name-${model.key}`}
                            value={model.name}
                            onChange={(event) =>
                              updateModelOverride(model.key, "name", {
                                name: event.target.value,
                              })
                            }
                            placeholder={t("pi.form.modelNamePlaceholder")}
                            aria-label={t("pi.form.modelName")}
                            className="min-w-0 flex-1"
                          />
                          <Button
                            type="button"
                            variant="ghost"
                            size="icon"
                            onClick={() => removeModel(model.key)}
                            aria-label={t("pi.form.removeModel")}
                            className="h-9 w-9 shrink-0 text-muted-foreground hover:text-destructive"
                          >
                            <Trash2 className="h-4 w-4" />
                          </Button>
                        </div>

                        {expandedModelKeys.has(model.key) && (
                          <div className="ml-9 grid gap-3 border-l border-border-default pl-4 sm:grid-cols-2">
                            <div className="flex min-h-9 flex-wrap items-center gap-x-8 gap-y-2 sm:col-span-2">
                              <div className="flex items-center gap-2.5">
                                <Label
                                  htmlFor={`pi-model-reasoning-${model.key}`}
                                  className="cursor-pointer"
                                >
                                  {t("pi.form.reasoning")}
                                </Label>
                                <Switch
                                  id={`pi-model-reasoning-${model.key}`}
                                  checked={model.reasoning === true}
                                  onCheckedChange={(checked) =>
                                    updateModelOverride(
                                      model.key,
                                      "reasoning",
                                      {
                                        reasoning: checked,
                                      },
                                    )
                                  }
                                  aria-describedby={metadataStatusId}
                                />
                              </div>
                              <div className="flex items-center gap-2.5">
                                <Label
                                  htmlFor={`pi-model-image-input-${model.key}`}
                                  className="cursor-pointer"
                                >
                                  {t("pi.form.imageInput")}
                                </Label>
                                <Switch
                                  id={`pi-model-image-input-${model.key}`}
                                  checked={supportsImageInput(model.input)}
                                  onCheckedChange={(checked) =>
                                    updateModelOverride(
                                      model.key,
                                      "imageInput",
                                      {
                                        input: withImageInput(
                                          model.input,
                                          checked,
                                        ),
                                      },
                                    )
                                  }
                                  aria-describedby={metadataStatusId}
                                />
                              </div>
                              <div className="ml-auto flex min-w-0 items-center gap-1.5 text-xs text-muted-foreground">
                                {isModelMetadataLoading &&
                                  !model.autoMetadata &&
                                  model.id && (
                                    <Loader2 className="h-3.5 w-3.5 shrink-0 animate-spin" />
                                  )}
                                <span id={metadataStatusId}>
                                  {t(
                                    modelMetadataStatusKey(
                                      model,
                                      isModelMetadataLoading,
                                      modelMetadataLookupComplete,
                                    ),
                                  )}
                                </span>
                                {canRestoreAutofill && (
                                  <Button
                                    type="button"
                                    variant="ghost"
                                    size="sm"
                                    onClick={() =>
                                      restoreModelAutofill(model.key)
                                    }
                                    className="h-7 shrink-0 px-2 text-xs"
                                  >
                                    {t("pi.form.restoreModelAutofill")}
                                  </Button>
                                )}
                              </div>
                            </div>
                            <Field
                              label={t("pi.form.contextWindow")}
                              htmlFor={`pi-model-context-window-${model.key}`}
                            >
                              <Input
                                id={`pi-model-context-window-${model.key}`}
                                type="number"
                                step="any"
                                inputMode="decimal"
                                value={model.contextWindow}
                                onChange={(event) =>
                                  updateModelOverride(
                                    model.key,
                                    "contextWindow",
                                    {
                                      contextWindow: event.target.value,
                                    },
                                  )
                                }
                                placeholder="128000"
                                aria-describedby={`pi-model-limits-hint-${model.key}`}
                              />
                            </Field>
                            <Field
                              label={t("pi.form.maxTokens")}
                              htmlFor={`pi-model-max-tokens-${model.key}`}
                            >
                              <Input
                                id={`pi-model-max-tokens-${model.key}`}
                                type="number"
                                step="any"
                                inputMode="decimal"
                                value={model.maxTokens}
                                onChange={(event) =>
                                  updateModelOverride(model.key, "maxTokens", {
                                    maxTokens: event.target.value,
                                  })
                                }
                                placeholder="16384"
                                aria-describedby={`pi-model-limits-hint-${model.key}`}
                              />
                            </Field>
                            <p
                              id={`pi-model-limits-hint-${model.key}`}
                              className="text-xs text-muted-foreground sm:col-span-2"
                            >
                              {t("pi.form.modelLimitsHint")}
                            </p>
                          </div>
                        )}
                      </div>
                    );
                  })}
                </div>
              )}

              <p className="text-xs text-muted-foreground">
                {t("pi.form.modelEditorHint", {
                  defaultValue: "配置可用的模型及其显示名称。",
                })}
              </p>
            </div>

            <RequestHeadersEditor
              headers={providerHeaders}
              onHeadersChange={setProviderHeaders}
              compact
            />

            <div className="space-y-2">
              <Label htmlFor="pi-config-preview">
                {t("provider.configJson")}
              </Label>
              <JsonEditor
                id="pi-config-preview"
                value={settingsConfigPreview}
                onChange={() => {}}
                height={Math.min(
                  360,
                  Math.max(
                    112,
                    settingsConfigPreview.split("\n").length * 20 + 20,
                  ),
                )}
                showValidation={false}
                language="json"
                darkMode={isDarkMode}
                readOnly
              />
            </div>
          </>
        )}

        {showButtons && (
          <div className="flex justify-end gap-2">
            <Button type="button" variant="outline" onClick={onCancel}>
              {t("common.cancel")}
            </Button>
            <Button type="submit" disabled={!isSubmitReady}>
              {submitLabel}
            </Button>
          </div>
        )}
      </form>
    </Form>
  );
}

function Field({
  label,
  htmlFor,
  children,
}: {
  label: string;
  htmlFor: string;
  children: React.ReactNode;
}) {
  return (
    <div className="space-y-1.5">
      <Label htmlFor={htmlFor}>{label}</Label>
      {children}
    </div>
  );
}
