import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { parse as parseYaml, stringify as stringifyYaml } from "yaml";
import {
  ChevronDown,
  ChevronRight,
  Download,
  Loader2,
  Plus,
  Trash2,
} from "lucide-react";
import { useForm } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import {
  Form,
  FormField,
  FormItem,
  FormLabel,
  FormMessage,
} from "@/components/ui/form";
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
import { BasicFormFields } from "./BasicFormFields";
import { ProviderPresetSelector } from "./ProviderPresetSelector";
import { RequestHeadersEditor } from "./RequestHeadersEditor";
import { StructuredOptionsEditor } from "./StructuredOptionsEditor";
import { ApiKeySection, EndpointField, ModelDropdown } from "./shared";
import {
  findRequestHeaderValue,
  normalizeRequestHeaders,
} from "./helpers/requestHeaders";
import {
  fetchModelsForConfig,
  showFetchModelsError,
  type FetchedModel,
} from "@/lib/api/model-fetch";
import { providerSchema, type ProviderFormData } from "@/lib/schemas/provider";
import { ohmypiProviderPresets } from "@/config/ohmypiProviderPresets";

import type { ProviderCategory } from "@/types";

// Allowed `api` values per ohmypi models.yml (see oh-my-pi docs/models.md).
const API_FORMATS = [
  { value: "openai-completions", label: "OpenAI Chat Completions" },
  { value: "openai-responses", label: "OpenAI Responses" },
  { value: "openai-codex-responses", label: "OpenAI Codex Responses" },
  { value: "azure-openai-responses", label: "Azure OpenAI Responses" },
  { value: "anthropic-messages", label: "Anthropic Messages" },
  { value: "bedrock-converse-stream", label: "Amazon Bedrock" },
  { value: "google-generative-ai", label: "Google Generative AI" },
  { value: "google-gemini-cli", label: "Google Gemini CLI" },
  { value: "google-vertex", label: "Google Vertex" },
] as const satisfies ReadonlyArray<{ value: string; label: string }>;

// Root-level keys owned by the curated form controls. Everything else in the
// provider's settingsConfig is treated as an unmanaged passthrough and
// preserved verbatim through the YAML escape hatch.
const ROOT_CONTROLLED_KEYS: Record<string, true> = {
  name: true,
  baseUrl: true,
  api: true,
  apiKey: true,
  headers: true,
  compat: true,
  models: true,
};

// Per-model keys exposed by the curated model rows (omp model overrides).
const MODEL_CONTROLLED_KEYS: Record<string, true> = {
  id: true,
  name: true,
  reasoning: true,
  thinking: true,
  input: true,
  imageInputDecoder: true,
  contextWindow: true,
  maxTokens: true,
  cost: true,
};

interface OhMyPiModelDraft {
  key: string;
  id: string;
  name: string;
  hasName: boolean;
  reasoning: boolean;
  hasReasoning: boolean;
  thinking: string;
  hasThinking: boolean;
  input: string[];
  hasInput: boolean;
  imageInputDecoder: string;
  hasImageInputDecoder: boolean;
  contextWindow: string;
  hasContextWindow: boolean;
  maxTokens: string;
  hasMaxTokens: boolean;
  cost: string;
  hasCost: boolean;
  passthrough: Record<string, unknown>;
}

class OhMyPiFormValidationError extends Error {
  constructor(
    message: string,
    readonly fieldSelector?: string,
    readonly modelKey?: string,
  ) {
    super(message);
    this.name = "OhMyPiFormValidationError";
  }
}

function validateOhMyPiField<T>(operation: () => T, fieldSelector: string): T {
  try {
    return operation();
  } catch (error) {
    throw new OhMyPiFormValidationError(
      error instanceof Error ? error.message : String(error),
      fieldSelector,
    );
  }
}

function objectWithout(
  value: Record<string, unknown>,
  denied: Record<string, true>,
): Record<string, unknown> {
  const result: Record<string, unknown> = {};
  for (const [key, val] of Object.entries(value)) {
    if (!denied[key]) result[key] = val;
  }
  return result;
}

function asObject(value: unknown): Record<string, unknown> {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : {};
}

function parseYamlObject(value: string): Record<string, unknown> | null {
  try {
    const parsed: unknown = parseYaml(value);
    return parsed && typeof parsed === "object" && !Array.isArray(parsed)
      ? (parsed as Record<string, unknown>)
      : null;
  } catch {
    return null;
  }
}

function parseOptionValue(value: string): unknown {
  try {
    return JSON.parse(value);
  } catch {
    return value;
  }
}

function formatOptionValue(value: unknown): string {
  if (typeof value === "string") return value;
  try {
    return JSON.stringify(value);
  } catch {
    return String(value);
  }
}

function optionalText(value: unknown): string {
  return typeof value === "string" ? value : "";
}

function hasOwn(value: Record<string, unknown>, key: string): boolean {
  return Object.prototype.hasOwnProperty.call(value, key);
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

function positiveNumber(
  value: string,
  errorMessage: string,
  fieldSelector: string,
): number {
  const parsed = Number(value);
  if (value.trim() === "" || !Number.isFinite(parsed) || parsed <= 0) {
    throw new OhMyPiFormValidationError(errorMessage, fieldSelector);
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

function normalizeProviderKey(value: string): string {
  return value.toLowerCase().replace(/[^a-z0-9-]/g, "");
}

function modelDraft(
  value: unknown,
  options: {
    key?: string;
  } = {},
): OhMyPiModelDraft {
  const model = asObject(value);
  return {
    key: options.key ?? crypto.randomUUID(),
    id: optionalText(model.id),
    name: optionalText(model.name),
    hasName: hasOwn(model, "name"),
    reasoning: model.reasoning === true,
    hasReasoning: hasOwn(model, "reasoning"),
    thinking: hasOwn(model, "thinking")
      ? formatOptionValue(model.thinking)
      : "",
    hasThinking: hasOwn(model, "thinking"),
    input: Array.isArray(model.input) ? [...model.input] : ["text"],
    hasInput: hasOwn(model, "input"),
    imageInputDecoder: hasOwn(model, "imageInputDecoder")
      ? formatOptionValue(model.imageInputDecoder)
      : "",
    hasImageInputDecoder: hasOwn(model, "imageInputDecoder"),
    contextWindow:
      typeof model.contextWindow === "number" &&
      Number.isFinite(model.contextWindow)
        ? String(model.contextWindow)
        : "",
    hasContextWindow: hasOwn(model, "contextWindow"),
    maxTokens:
      typeof model.maxTokens === "number" && Number.isFinite(model.maxTokens)
        ? String(model.maxTokens)
        : "",
    hasMaxTokens: hasOwn(model, "maxTokens"),
    cost: hasOwn(model, "cost") ? formatOptionValue(model.cost) : "",
    hasCost: hasOwn(model, "cost"),
    passthrough: objectWithout(model, MODEL_CONTROLLED_KEYS),
  };
}

function newModel(): OhMyPiModelDraft {
  return {
    key: crypto.randomUUID(),
    id: "",
    name: "",
    hasName: true,
    reasoning: false,
    hasReasoning: true,
    thinking: "",
    hasThinking: true,
    input: ["text"],
    hasInput: true,
    imageInputDecoder: "",
    hasImageInputDecoder: true,
    contextWindow: "",
    hasContextWindow: true,
    maxTokens: "",
    hasMaxTokens: true,
    cost: "",
    hasCost: true,
    passthrough: {},
  };
}

function modelPreview(model: OhMyPiModelDraft): Record<string, unknown> {
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
    ...(model.hasName && displayName ? { name: displayName } : {}),
    ...(model.hasReasoning ? { reasoning: model.reasoning } : {}),
    ...(model.hasThinking && model.thinking.trim()
      ? { thinking: parseOptionValue(model.thinking.trim()) }
      : {}),
    ...(model.hasInput
      ? { input: withImageInput(model.input, supportsImageInput(model.input)) }
      : {}),
    ...(model.hasImageInputDecoder && model.imageInputDecoder.trim()
      ? { imageInputDecoder: model.imageInputDecoder.trim() }
      : {}),
    ...(model.hasContextWindow && contextWindow !== undefined
      ? { contextWindow }
      : {}),
    ...(model.hasMaxTokens && maxTokens !== undefined ? { maxTokens } : {}),
    ...(model.hasCost && model.cost.trim()
      ? { cost: parseOptionValue(model.cost.trim()) }
      : {}),
  };
}

function buildOhMyPiSettingsConfig({
  passthrough,
  name,
  baseUrl,
  api,
  apiKey,
  headers,
  compat,
  includeCompat,
  models,
  includeModels,
}: {
  passthrough: Record<string, unknown>;
  name?: string;
  baseUrl: string;
  api: string;
  apiKey: string;
  headers: Record<string, string>;
  compat: Record<string, unknown>;
  includeCompat: boolean;
  models: Record<string, unknown>[];
  includeModels: boolean;
}): Record<string, unknown> {
  return {
    ...passthrough,
    ...(name !== undefined && name.trim() ? { name: name.trim() } : {}),
    ...(baseUrl.trim() ? { baseUrl: baseUrl.trim() } : {}),
    ...(api.trim() ? { api: api.trim() } : {}),
    ...(apiKey ? { apiKey } : {}),
    ...(Object.keys(headers).length > 0 ? { headers } : {}),
    ...(includeCompat ? { compat } : {}),
    ...(includeModels ? { models } : {}),
  };
}

export function OhMyPiProviderForm({
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
  const initialConfig = useMemo(
    () => asObject(initialData?.settingsConfig),
    [initialData?.settingsConfig],
  );
  const isEdit = Boolean(initialData);
  const initialDisplayName =
    initialData?.name ?? optionalText(initialConfig.name);

  const [selectedPresetId, setSelectedPresetId] = useState<string | null>(
    isEdit ? null : "custom",
  );
  const [category, setCategory] = useState<ProviderCategory>(
    initialData?.category ?? "custom",
  );
  const [providerKey, setProviderKey] = useState(providerId ?? "");
  const [baseUrl, setBaseUrl] = useState(optionalText(initialConfig.baseUrl));
  const [api, setApi] = useState(
    () => optionalText(initialConfig.api) || "openai-completions",
  );
  const [apiKey, setApiKey] = useState(optionalText(initialConfig.apiKey));
  const [providerHeaders, setProviderHeaders] = useState<
    Record<string, string>
  >(() => {
    const initialHeaders: Record<string, string> = {};
    for (const [key, val] of Object.entries(asObject(initialConfig.headers))) {
      if (typeof val === "string") initialHeaders[key] = val;
    }
    return initialHeaders;
  });
  const [providerCompat, setProviderCompat] = useState<Record<string, unknown>>(
    () => asObject(initialConfig.compat),
  );
  const includeCompatRef = useRef(hasOwn(initialConfig, "compat"));
  const [providerPassthrough, setProviderPassthrough] = useState<
    Record<string, unknown>
  >(() => objectWithout(initialConfig, ROOT_CONTROLLED_KEYS));
  const [passthroughText, setPassthroughText] = useState(() => {
    const passthrough = objectWithout(initialConfig, ROOT_CONTROLLED_KEYS);
    return Object.keys(passthrough).length > 0
      ? stringifyYaml(passthrough, { indent: 2 })
      : "";
  });
  const [includeModels, setIncludeModels] = useState(
    () => !isEdit || hasOwn(initialConfig, "models"),
  );
  const initialModels = useMemo<OhMyPiModelDraft[]>(() => {
    if (!Array.isArray(initialConfig.models)) return [];
    return initialConfig.models.map((model) => modelDraft(model));
  }, [initialConfig.models]);
  const [models, setModels] = useState<OhMyPiModelDraft[]>(initialModels);
  const [expandedModelKeys, setExpandedModelKeys] = useState<Set<string>>(
    () => new Set(),
  );
  const [fetchedModels, setFetchedModels] = useState<FetchedModel[] | null>(
    null,
  );
  const [isFetchingModels, setIsFetchingModels] = useState(false);
  const modelFetchGenerationRef = useRef(0);
  const [showAdvanced, setShowAdvanced] = useState(false);
  const [formError, setFormError] = useState<string | null>(null);

  const form = useForm<ProviderFormData>({
    resolver: zodResolver(providerSchema),
    mode: "onChange",
    defaultValues: {
      name: initialData?.name ?? optionalText(initialConfig.name),
      websiteUrl: initialData?.websiteUrl ?? "",
      notes: initialData?.notes ?? "",
      settingsConfig: JSON.stringify(initialConfig, null, 2),
      icon: initialData?.icon === "pi" ? "omp" : (initialData?.icon ?? ""),
      iconColor: initialData?.iconColor ?? "",
    },
  });

  const hasConfigurationSelection = isEdit || selectedPresetId !== null;
  const isSettingsConfigValid =
    passthroughText.trim() === "" || parseYamlObject(passthroughText) !== null;
  const isSubmitReady =
    form.watch("name").trim().length > 0 && isSettingsConfigValid;

  useEffect(() => {
    onSubmitReadyChange?.(isSubmitReady);
  }, [isSubmitReady, onSubmitReadyChange]);

  const buildSettingsConfig = useCallback(
    (displayName: string): Record<string, unknown> => {
      const parsedPassthrough = parseYamlObject(passthroughText);
      const passthrough = parsedPassthrough ?? providerPassthrough;
      return buildOhMyPiSettingsConfig({
        passthrough,
        name: displayName,
        baseUrl,
        api,
        apiKey,
        headers: normalizeRequestHeaders(providerHeaders),
        compat: providerCompat,
        includeCompat: includeCompatRef.current,
        models: models
          .filter((model) => model.id.trim())
          .map((model) => modelPreview({ ...model, id: model.id.trim() })),
        includeModels,
      });
    },
    [
      api,
      apiKey,
      baseUrl,
      includeModels,
      models,
      passthroughText,
      providerCompat,
      providerHeaders,
      providerPassthrough,
    ],
  );

  const syncSettingsConfig = useCallback(() => {
    const displayName = form.getValues("name");
    const config = buildSettingsConfig(
      typeof displayName === "string" ? displayName : "",
    );
    form.setValue("settingsConfig", JSON.stringify(config, null, 2), {
      shouldDirty: true,
      shouldValidate: true,
    });
  }, [buildSettingsConfig, form]);

  // Keep the hidden settingsConfig field in sync with the curated controls.
  useEffect(() => {
    syncSettingsConfig();
  }, [
    baseUrl,
    api,
    apiKey,
    providerHeaders,
    providerCompat,
    passthroughText,
    includeModels,
    models,
    syncSettingsConfig,
  ]);

  const invalidateFetchedModels = useCallback(() => {
    modelFetchGenerationRef.current += 1;
    setFetchedModels(null);
    setIsFetchingModels(false);
  }, []);

  useEffect(
    () => () => {
      modelFetchGenerationRef.current += 1;
    },
    [],
  );

  const presetEntries = useMemo(
    () =>
      ohmypiProviderPresets.map((preset, index) => ({
        id: `ohmypi-${index}`,
        preset,
      })),
    [],
  );

  const selectPreset = (id: string) => {
    setFormError(null);
    setSelectedPresetId(id);
    setCategory("custom");
    setProviderKey("");
    setBaseUrl("");
    setApi("openai-completions");
    setApiKey("");
    setProviderHeaders({});
    setProviderCompat({});
    includeCompatRef.current = false;
    setProviderPassthrough({});
    setPassthroughText("");
    setIncludeModels(true);
    setModels([]);
    setExpandedModelKeys(new Set());
    invalidateFetchedModels();
    form.reset({
      name: initialDisplayName,
      websiteUrl: "",
      notes: "",
      settingsConfig: JSON.stringify(
        buildOhMyPiSettingsConfig({
          passthrough: {},
          name: initialDisplayName,
          baseUrl: "",
          api: "openai-completions",
          apiKey: "",
          headers: {},
          compat: {},
          includeCompat: false,
          models: [],
          includeModels: true,
        }),
        null,
        2,
      ),
      icon: initialData?.icon === "pi" ? "omp" : (initialData?.icon ?? ""),
      iconColor: initialData?.iconColor ?? "",
    });
    if (id === "custom") return;
    const entry = presetEntries.find((candidate) => candidate.id === id);
    if (!entry) return;
    const preset = entry.preset;
    const presetConfig = asObject(preset.settingsConfig);
    const headers: Record<string, string> = {};
    for (const [key, val] of Object.entries(asObject(presetConfig.headers))) {
      if (typeof val === "string") headers[key] = val;
    }
    setCategory(preset.category ?? "custom");
    setProviderKey(preset.providerKey);
    setBaseUrl(preset.settingsConfig.baseUrl);
    setApi(preset.settingsConfig.api);
    setIncludeModels(true);
    setProviderHeaders(headers);
    setProviderCompat(asObject(presetConfig.compat));
    includeCompatRef.current = hasOwn(presetConfig, "compat");
    const passthrough = objectWithout(presetConfig, ROOT_CONTROLLED_KEYS);
    setProviderPassthrough(passthrough);
    setPassthroughText(
      Object.keys(passthrough).length > 0 ? stringifyYaml(passthrough) : "",
    );
    setModels(preset.settingsConfig.models.map((model) => modelDraft(model)));
    setExpandedModelKeys(new Set());
    form.reset({
      name: preset.settingsConfig.name,
      websiteUrl: preset.websiteUrl,
      notes: "",
      settingsConfig: JSON.stringify(presetConfig, null, 2),
      icon: preset.icon ?? "",
      iconColor: preset.iconColor ?? "",
    });
  };

  const handleApiChange = useCallback(
    (value: string) => {
      setApi(value);
      invalidateFetchedModels();
    },
    [invalidateFetchedModels],
  );

  const handleApiKeyChange = useCallback(
    (value: string) => {
      setApiKey(value);
      invalidateFetchedModels();
    },
    [invalidateFetchedModels],
  );

  const handleBaseUrlChange = useCallback(
    (value: string) => {
      setBaseUrl(value);
      invalidateFetchedModels();
    },
    [invalidateFetchedModels],
  );

  const handleProviderHeadersChange = useCallback(
    (value: Record<string, string>) => {
      setProviderHeaders(value);
      invalidateFetchedModels();
    },
    [invalidateFetchedModels],
  );

  const handleProviderCompatChange = useCallback(
    (value: Record<string, unknown>) => {
      includeCompatRef.current = Object.keys(value).length > 0;
      setProviderCompat(value);
    },
    [],
  );

  const handleProviderKeyChange = useCallback((value: string) => {
    setProviderKey(normalizeProviderKey(value));
  }, []);

  const handlePassthroughChange = (value: string) => {
    setPassthroughText(value);
    const parsed = parseYamlObject(value);
    if (parsed) setProviderPassthrough(parsed);
  };

  const changeModelId = (key: string, id: string) => {
    setModels((current) =>
      current.map((model) =>
        model.key === key
          ? {
              ...model,
              id,
              name:
                model.hasName &&
                (model.name.length === 0 || model.name === model.id)
                  ? id
                  : model.name,
            }
          : model,
      ),
    );
  };

  const updateModelOverride = (
    key: string,
    update: Partial<Omit<OhMyPiModelDraft, "key">>,
  ) => {
    setModels((current) =>
      current.map((model) =>
        model.key === key ? { ...model, ...update } : model,
      ),
    );
  };

  const addModel = () => {
    const model = newModel();
    setIncludeModels(true);
    setModels((current) => [...current, model]);
  };

  const removeModel = (key: string) => {
    setModels((current) => current.filter((model) => model.key !== key));
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

    const requestGeneration = ++modelFetchGenerationRef.current;
    setFetchedModels(null);
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
        if (modelFetchGenerationRef.current !== requestGeneration) return;
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
        if (modelFetchGenerationRef.current !== requestGeneration) return;
        console.warn("[ModelFetch] Failed:", error);
        showFetchModelsError(error, t);
      })
      .finally(() => {
        if (modelFetchGenerationRef.current === requestGeneration) {
          setIsFetchingModels(false);
        }
      });
  }, [api, apiKey, baseUrl, providerHeaders, t]);

  const submit = async (identity: ProviderFormData) => {
    onSubmittingChange?.(true);
    setFormError(null);
    try {
      const trimmedName = identity.name.trim();
      const trimmedKey = normalizeProviderKey(
        isEdit ? (providerId ?? "") : providerKey || trimmedName,
      );
      if (!trimmedName) {
        throw new OhMyPiFormValidationError(
          t("ohmypi.form.nameRequired"),
          'input[name="name"]',
        );
      }
      if (!isEdit && !trimmedKey) {
        throw new OhMyPiFormValidationError(
          t("ohmypi.form.providerKeyRequired"),
          "#ohmypi-provider-key",
        );
      }
      const seen = new Set<string>();
      for (const model of models) {
        const id = model.id.trim();
        if (id.length === 0) {
          throw new OhMyPiFormValidationError(
            t("ohmypi.form.modelIdRequired", {
              index: models.indexOf(model) + 1,
            }),
            `#ohmypi-model-id-${model.key}`,
            model.key,
          );
        }
        if (seen.has(id)) {
          throw new OhMyPiFormValidationError(
            t("ohmypi.form.duplicateModel", { id }),
            `#ohmypi-model-id-${model.key}`,
            model.key,
          );
        }
        seen.add(id);
        if (model.contextWindow.trim() !== "") {
          validateOhMyPiField(
            () =>
              positiveNumber(
                model.contextWindow,
                t("ohmypi.form.positiveNumberRequired", {
                  label: t("ohmypi.form.contextWindow"),
                }),
                `#ohmypi-model-context-window-${model.key}`,
              ),
            `#ohmypi-model-context-window-${model.key}`,
          );
        }
        if (model.maxTokens.trim() !== "") {
          validateOhMyPiField(
            () =>
              positiveNumber(
                model.maxTokens,
                t("ohmypi.form.positiveNumberRequired", {
                  label: t("ohmypi.form.maxTokens"),
                }),
                `#ohmypi-model-max-tokens-${model.key}`,
              ),
            `#ohmypi-model-max-tokens-${model.key}`,
          );
        }
      }

      if (baseUrl.trim()) {
        validateOhMyPiField(
          () =>
            validateAbsoluteHttpUrl(
              baseUrl.trim(),
              t("ohmypi.form.absoluteHttpUrlRequired", {
                label: t("opencode.baseUrl", { defaultValue: "Base URL" }),
              }),
            ),
          "#ohmypi-provider-base-url",
        );
      }

      const settingsConfig = buildSettingsConfig(trimmedName);
      const values: ProviderFormValues = {
        name: trimmedName,
        websiteUrl: identity.websiteUrl?.trim() ?? "",
        notes: identity.notes?.trim() ?? "",
        settingsConfig: JSON.stringify(settingsConfig),
        icon: identity.icon === "pi" ? "omp" : identity.icon || "omp",
        iconColor: identity.iconColor || "",
        providerKey: isEdit ? providerId : trimmedKey,
        presetId: selectedPresetId ?? undefined,
        presetCategory: category,
        meta: initialData?.meta,
      };
      await onSubmit(values);
    } catch (error) {
      const rawMessage = error instanceof Error ? error.message : String(error);
      setFormError(rawMessage);
      if (error instanceof OhMyPiFormValidationError) {
        const modelKey =
          error.modelKey ??
          error.fieldSelector?.match(/#ohmypi-model-([0-9a-f-]{36})$/)?.[1];
        if (modelKey) {
          setExpandedModelKeys((current) => {
            const next = new Set(current);
            next.add(modelKey);
            return next;
          });
        }
        if (error.fieldSelector) {
          requestAnimationFrame(() => {
            document.querySelector<HTMLElement>(error.fieldSelector!)?.focus();
          });
        }
        toast.error(rawMessage);
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
  const isKnownApiFormat = API_FORMATS.some((format) => format.value === api);

  return (
    <Form {...form}>
      <form
        id="provider-form"
        onSubmit={form.handleSubmit(submit)}
        noValidate
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

        {hasConfigurationSelection && !isSettingsConfigValid && (
          <p
            role="status"
            className="rounded-lg border border-amber-500/30 bg-amber-500/10 px-4 py-3 text-sm text-amber-900 dark:text-amber-200"
          >
            {t("ohmypi.form.fixYamlFirst")}
          </p>
        )}

        {hasConfigurationSelection && (
          <fieldset
            disabled={!isSettingsConfigValid}
            className="min-w-0 space-y-6 border-0 p-0 disabled:opacity-50"
          >
            <BasicFormFields
              form={form}
              beforeNameSlot={
                isEdit || selectedPresetId === "custom" ? (
                  <div className="space-y-2">
                    <Label htmlFor="ohmypi-provider-key">
                      {t("ohmypi.form.providerKey")}
                      <span
                        aria-hidden="true"
                        className="text-destructive ml-1"
                      >
                        *
                      </span>
                    </Label>
                    <Input
                      id="ohmypi-provider-key"
                      value={providerKey}
                      onChange={(event) =>
                        handleProviderKeyChange(event.target.value)
                      }
                      disabled={isEdit}
                      placeholder="my-provider"
                      autoComplete="off"
                    />
                    <p className="text-xs text-muted-foreground">
                      {isEdit
                        ? t("opencode.providerKeyLockedHint", {
                            defaultValue:
                              "该供应商已添加到应用配置中，供应商标识不可修改",
                          })
                        : t("opencode.providerKeyHint", {
                            defaultValue:
                              "配置文件中的唯一标识符，只能使用小写字母、数字和连字符",
                          })}
                    </p>
                  </div>
                ) : undefined
              }
            />

            <Field
              label={t("opencode.npmPackage", {
                defaultValue: "接口格式",
              })}
              htmlFor="ohmypi-provider-api-select"
            >
              <Select value={api} onValueChange={handleApiChange}>
                <SelectTrigger
                  id="ohmypi-provider-api-select"
                  className="w-full"
                >
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {API_FORMATS.map((format) => (
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
                {t("opencode.npmPackageHint", {
                  defaultValue: "选择 AI 服务的 API 接口格式",
                })}
              </p>
            </Field>

            <ApiKeySection
              id="ohmypi-api-key"
              label={t("ohmypi.form.credential")}
              value={apiKey}
              onChange={handleApiKeyChange}
              category={category}
              shouldShowLink={false}
              websiteUrl=""
            />

            <div className="space-y-2">
              <EndpointField
                id="ohmypi-provider-base-url"
                label={t("opencode.baseUrl", { defaultValue: "Base URL" })}
                value={baseUrl}
                onChange={handleBaseUrlChange}
                placeholder="https://api.example.com/v1"
              />
              <p className="text-xs text-muted-foreground">
                {t("opencode.baseUrlHint", {
                  defaultValue: "自定义 API 端点地址",
                })}
              </p>
            </div>

            <RequestHeadersEditor
              headers={providerHeaders}
              onHeadersChange={handleProviderHeadersChange}
            />

            <StructuredOptionsEditor
              id="ohmypi-provider-compat"
              title={t("ohmypi.form.compatibility")}
              hint={t("ohmypi.form.compatibilityHint")}
              addLabel={t("ohmypi.form.addCompatibilityOption")}
              emptyLabel={t("ohmypi.form.noCompatibilityOptions")}
              keyLabel={t("ohmypi.form.optionKey")}
              valueLabel={t("ohmypi.form.optionValue")}
              keyPlaceholder="supportsDeveloperRole"
              valuePlaceholder="false"
              removeLabel={t("ohmypi.form.removeCompatibilityOption")}
              options={providerCompat}
              onOptionsChange={handleProviderCompatChange}
            />

            <div className="flex items-center gap-2">
              <Switch
                checked={includeModels}
                onCheckedChange={setIncludeModels}
                id="ohmypi-models-toggle"
              />
              <Label htmlFor="ohmypi-models-toggle">
                {t("ohmypi.form.modelsToggle", {
                  defaultValue: "配置模型列表",
                })}
              </Label>
            </div>

            <div
              id="ohmypi-models-section"
              tabIndex={-1}
              className="space-y-3 border-l border-border-default pl-3 outline-none"
            >
              <div className="flex items-center justify-between gap-3">
                <FormLabel>
                  {t("opencode.models", { defaultValue: "模型配置" })}
                </FormLabel>
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
                    id="ohmypi-add-model"
                    type="button"
                    variant="outline"
                    size="sm"
                    onClick={addModel}
                    className="h-7 gap-1"
                  >
                    <Plus className="h-3.5 w-3.5" />
                    {t("ohmypi.form.addModel")}
                  </Button>
                </div>
              </div>

              {!includeModels ? (
                <p role="status" className="py-2 text-sm text-muted-foreground">
                  {t("ohmypi.form.modelsHiddenHint", {
                    defaultValue:
                      "未启用模型列表：保存后将以 override-only 形态写入（不写 models 键）",
                  })}
                </p>
              ) : models.length === 0 ? (
                <p role="status" className="py-2 text-sm text-muted-foreground">
                  {t("ohmypi.form.noModels", {
                    defaultValue: "暂无模型配置",
                  })}
                </p>
              ) : (
                <div className="space-y-2">
                  <div className="flex items-center gap-2 px-1 text-xs text-muted-foreground">
                    <span className="w-9" />
                    <span className="flex-1">
                      {t("ohmypi.form.modelId")}
                      <span
                        aria-hidden="true"
                        className="ml-1 text-destructive"
                      >
                        *
                      </span>
                    </span>
                    <span className="flex-1">{t("ohmypi.form.modelName")}</span>
                    <span className="w-9" />
                  </div>
                  {models.map((model) => {
                    const isExpanded = expandedModelKeys.has(model.key);
                    return (
                      <div key={model.key} className="space-y-2">
                        <div className="flex items-center gap-2">
                          <Button
                            type="button"
                            variant="ghost"
                            size="icon"
                            onClick={() => toggleModelDetails(model.key)}
                            aria-label={t("ohmypi.form.toggleModelDetails", {
                              defaultValue: "展开或收起模型详情",
                            })}
                            className="h-9 w-9 shrink-0"
                          >
                            <ChevronRight
                              className={`h-4 w-4 transition-transform motion-reduce:transition-none ${
                                isExpanded ? "rotate-90" : ""
                              }`}
                            />
                          </Button>
                          <div className="flex min-w-0 flex-1 gap-1">
                            <Input
                              id={`ohmypi-model-id-${model.key}`}
                              value={model.id}
                              onChange={(event) =>
                                changeModelId(model.key, event.target.value)
                              }
                              placeholder="model-id"
                              aria-label={t("ohmypi.form.modelId")}
                              required
                              className="min-w-0 flex-1"
                            />
                            {fetchedModels && fetchedModels.length > 0 && (
                              <ModelDropdown
                                models={fetchedModels}
                                onSelect={(id) => changeModelId(model.key, id)}
                              />
                            )}
                          </div>
                          <Input
                            id={`ohmypi-model-name-${model.key}`}
                            value={model.name}
                            onChange={(event) =>
                              updateModelOverride(model.key, {
                                name: event.target.value,
                                hasName: true,
                              })
                            }
                            placeholder={t("ohmypi.form.modelNamePlaceholder")}
                            aria-label={t("ohmypi.form.modelName")}
                            className="min-w-0 flex-1"
                          />
                          <Button
                            type="button"
                            variant="ghost"
                            size="icon"
                            onClick={() => removeModel(model.key)}
                            aria-label={t("ohmypi.form.removeModel")}
                            className="h-9 w-9 shrink-0 text-muted-foreground hover:text-destructive"
                          >
                            <Trash2 className="h-4 w-4" />
                          </Button>
                        </div>

                        {isExpanded && (
                          <div className="ml-9 grid gap-3 border-l-2 border-muted pl-4 sm:grid-cols-2">
                            <div className="flex min-h-9 flex-wrap items-center gap-x-8 gap-y-2 sm:col-span-2">
                              <div className="flex items-center gap-2.5">
                                <Label
                                  htmlFor={`ohmypi-model-reasoning-${model.key}`}
                                  className="cursor-pointer"
                                >
                                  {t("ohmypi.form.reasoning")}
                                </Label>
                                <Switch
                                  id={`ohmypi-model-reasoning-${model.key}`}
                                  checked={model.reasoning}
                                  onCheckedChange={(checked) =>
                                    updateModelOverride(model.key, {
                                      reasoning: checked,
                                      hasReasoning: true,
                                    })
                                  }
                                />
                              </div>
                              <div className="flex items-center gap-2.5">
                                <Label
                                  htmlFor={`ohmypi-model-image-input-${model.key}`}
                                  className="cursor-pointer"
                                >
                                  {t("ohmypi.form.imageInput")}
                                </Label>
                                <Switch
                                  id={`ohmypi-model-image-input-${model.key}`}
                                  checked={supportsImageInput(model.input)}
                                  onCheckedChange={(checked) =>
                                    updateModelOverride(model.key, {
                                      input: withImageInput(
                                        model.input,
                                        checked,
                                      ),
                                      hasInput: true,
                                    })
                                  }
                                />
                              </div>
                            </div>
                            <Field
                              label={<>{t("ohmypi.form.thinking")}</>}
                              htmlFor={`ohmypi-model-thinking-${model.key}`}
                            >
                              <Input
                                id={`ohmypi-model-thinking-${model.key}`}
                                aria-label={t("ohmypi.form.thinking")}
                                value={model.thinking}
                                onChange={(event) =>
                                  updateModelOverride(model.key, {
                                    thinking: event.target.value,
                                    hasThinking: true,
                                  })
                                }
                                placeholder='{"mode":"effort"}'
                                className="font-mono text-xs"
                              />
                            </Field>
                            <Field
                              label={t("ohmypi.form.imageInputDecoder")}
                              htmlFor={`ohmypi-model-image-input-decoder-${model.key}`}
                            >
                              <Input
                                id={`ohmypi-model-image-input-decoder-${model.key}`}
                                aria-label={t("ohmypi.form.imageInputDecoder")}
                                value={model.imageInputDecoder}
                                onChange={(event) =>
                                  updateModelOverride(model.key, {
                                    imageInputDecoder: event.target.value,
                                    hasImageInputDecoder: true,
                                  })
                                }
                                placeholder="hex"
                                className="font-mono text-xs"
                              />
                            </Field>
                            <Field
                              label={t("ohmypi.form.contextWindow")}
                              htmlFor={`ohmypi-model-context-window-${model.key}`}
                            >
                              <Input
                                id={`ohmypi-model-context-window-${model.key}`}
                                aria-label={t("ohmypi.form.contextWindow")}
                                type="number"
                                step="any"
                                min="1"
                                inputMode="decimal"
                                value={model.contextWindow}
                                onChange={(event) =>
                                  updateModelOverride(model.key, {
                                    contextWindow: event.target.value,
                                    hasContextWindow: true,
                                  })
                                }
                                placeholder="128000"
                              />
                            </Field>
                            <Field
                              label={t("ohmypi.form.maxTokens")}
                              htmlFor={`ohmypi-model-max-tokens-${model.key}`}
                            >
                              <Input
                                id={`ohmypi-model-max-tokens-${model.key}`}
                                aria-label={t("ohmypi.form.maxTokens")}
                                type="number"
                                step="any"
                                min="1"
                                inputMode="decimal"
                                value={model.maxTokens}
                                onChange={(event) =>
                                  updateModelOverride(model.key, {
                                    maxTokens: event.target.value,
                                    hasMaxTokens: true,
                                  })
                                }
                                placeholder="16384"
                              />
                            </Field>
                            <Field
                              label={t("ohmypi.form.cost")}
                              htmlFor={`ohmypi-model-cost-${model.key}`}
                            >
                              <Input
                                id={`ohmypi-model-cost-${model.key}`}
                                aria-label={t("ohmypi.form.cost")}
                                value={model.cost}
                                onChange={(event) =>
                                  updateModelOverride(model.key, {
                                    cost: event.target.value,
                                    hasCost: true,
                                  })
                                }
                                placeholder='{"input":0.15,"output":0.25}'
                                className="font-mono text-xs"
                              />
                            </Field>
                          </div>
                        )}
                      </div>
                    );
                  })}
                </div>
              )}

              <p className="text-xs text-muted-foreground">
                {t("opencode.modelsHint", {
                  defaultValue: "配置可用的模型及其显示名称。",
                })}
              </p>
            </div>
          </fieldset>
        )}

        {hasConfigurationSelection && (
          <div className="space-y-2 rounded-lg border border-border-default p-4">
            <Button
              type="button"
              variant={null}
              size="sm"
              className="h-8 w-full justify-start gap-1.5 px-0 text-sm font-medium text-foreground hover:opacity-70"
              onClick={() => setShowAdvanced((current) => !current)}
              aria-expanded={showAdvanced}
            >
              {showAdvanced ? (
                <ChevronDown className="h-4 w-4" />
              ) : (
                <ChevronRight className="h-4 w-4" />
              )}
              {t("providerForm.advancedOptionsToggle", {
                defaultValue: "高级选项",
              })}
            </Button>
            {!showAdvanced && (
              <p className="mt-1 ml-1 text-xs text-muted-foreground">
                {t("ohmypi.form.advancedHint", {
                  defaultValue: "包含其他配置字段（YAML，原样保留）。",
                })}
              </p>
            )}
            {showAdvanced && (
              <FormField
                control={form.control}
                name="settingsConfig"
                render={() => (
                  <FormItem className="space-y-2">
                    <FormLabel>
                      {t("ohmypi.form.passthroughLabel", {
                        defaultValue: "其他字段（YAML，原样保留）",
                      })}
                    </FormLabel>
                    <textarea
                      className="min-h-32 w-full rounded-md border bg-transparent px-3 py-2 font-mono text-xs"
                      value={passthroughText}
                      onChange={(event) =>
                        handlePassthroughChange(event.target.value)
                      }
                      placeholder="disableStrictTools: false
discovery:
  type: proxy"
                      aria-label={t("ohmypi.form.passthroughLabel", {
                        defaultValue: "其他字段（YAML，原样保留）",
                      })}
                    />
                    <FormMessage />
                  </FormItem>
                )}
              />
            )}
          </div>
        )}

        {showButtons && (
          <div className="flex justify-end gap-2 pt-2">
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
  label: React.ReactNode;
  htmlFor: string;
  children: React.ReactNode;
}) {
  return (
    <div className="space-y-2">
      <Label htmlFor={htmlFor}>{label}</Label>
      {children}
    </div>
  );
}
