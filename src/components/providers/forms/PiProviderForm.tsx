import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
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
import {
  Popover,
  PopoverAnchor,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
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
import { StructuredOptionsEditor } from "./StructuredOptionsEditor";
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
  isPiThinkingLevelMap,
  PI_THINKING_LEVELS,
  type PiThinkingLevel,
  type PiThinkingLevelMap,
} from "@/config/piThinkingProfiles";
import {
  fetchModelsForConfig,
  showFetchModelsError,
  type FetchedModel,
} from "@/lib/api/model-fetch";
import { useDarkMode } from "@/hooks/useDarkMode";
import { providerSchema, type ProviderFormData } from "@/lib/schemas/provider";
import type { ProviderCategory } from "@/types";
import { translatePiProviderMutationError } from "@/utils/errorUtils";

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
  "compat",
  "models",
]);
const MODEL_CONTROLLED_KEYS = new Set([
  "id",
  "name",
  "reasoning",
  "input",
  "contextWindow",
  "maxTokens",
  "thinkingLevelMap",
]);

interface PiModelDraft {
  key: string;
  id: string;
  name: string;
  hasName: boolean;
  reasoning: boolean;
  hasReasoning: boolean;
  input: unknown;
  hasInput: boolean;
  contextWindow: string;
  hasContextWindow: boolean;
  maxTokens: string;
  hasMaxTokens: boolean;
  thinkingLevelMap: unknown;
  hasThinkingLevelMap: boolean;
  passthrough: Record<string, unknown>;
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

function parseJsonObject(value: string): Record<string, unknown> | null {
  try {
    const parsed: unknown = JSON.parse(value);
    return parsed && typeof parsed === "object" && !Array.isArray(parsed)
      ? (parsed as Record<string, unknown>)
      : null;
  } catch {
    return null;
  }
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

function jsonValuesEqual(left: unknown, right: unknown): boolean {
  if (Object.is(left, right)) return true;
  if (Array.isArray(left) && Array.isArray(right)) {
    return (
      left.length === right.length &&
      left.every((value, index) => jsonValuesEqual(value, right[index]))
    );
  }
  if (
    left &&
    right &&
    typeof left === "object" &&
    typeof right === "object" &&
    !Array.isArray(left) &&
    !Array.isArray(right)
  ) {
    const leftObject = left as Record<string, unknown>;
    const rightObject = right as Record<string, unknown>;
    const leftKeys = Object.keys(leftObject);
    const rightKeys = Object.keys(rightObject);
    return (
      leftKeys.length === rightKeys.length &&
      leftKeys.every(
        (key) =>
          hasOwn(rightObject, key) &&
          jsonValuesEqual(leftObject[key], rightObject[key]),
      )
    );
  }
  return false;
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

function positiveNumber(
  value: string,
  errorMessage: string,
  fieldSelector: string,
): number {
  const parsed = Number(value);
  if (value.trim() === "" || !Number.isFinite(parsed) || parsed <= 0) {
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

function modelDraft(
  value: unknown,
  options: {
    key?: string;
  } = {},
): PiModelDraft {
  const model = asObject(value);
  return {
    key: options.key ?? crypto.randomUUID(),
    id: optionalText(model.id),
    name: optionalText(model.name),
    hasName: hasOwn(model, "name"),
    reasoning: model.reasoning === true,
    hasReasoning: hasOwn(model, "reasoning"),
    input: Array.isArray(model.input) ? model.input : ["text"],
    hasInput: hasOwn(model, "input"),
    contextWindow: optionalNumberText(model.contextWindow),
    hasContextWindow: hasOwn(model, "contextWindow"),
    maxTokens: optionalNumberText(model.maxTokens),
    hasMaxTokens: hasOwn(model, "maxTokens"),
    thinkingLevelMap: model.thinkingLevelMap,
    hasThinkingLevelMap: hasOwn(model, "thinkingLevelMap"),
    passthrough: objectWithout(model, MODEL_CONTROLLED_KEYS),
  };
}

function newModel(): PiModelDraft {
  return {
    key: crypto.randomUUID(),
    id: "",
    name: "",
    hasName: true,
    reasoning: false,
    hasReasoning: true,
    input: ["text"],
    hasInput: true,
    contextWindow: "",
    hasContextWindow: true,
    maxTokens: "",
    hasMaxTokens: true,
    thinkingLevelMap: undefined,
    hasThinkingLevelMap: false,
    passthrough: {},
  };
}

type PiThinkingLevelMode = "default" | "unsupported" | "value";

function thinkingLevelMode(
  map: PiThinkingLevelMap,
  level: PiThinkingLevel,
): PiThinkingLevelMode {
  if (!hasOwn(map, level)) return "default";
  return map[level] === null ? "unsupported" : "value";
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
    ...(model.hasName ? { name: displayName } : {}),
    ...(model.hasReasoning ? { reasoning: model.reasoning } : {}),
    ...(model.hasInput
      ? { input: withImageInput(model.input, supportsImageInput(model.input)) }
      : {}),
    ...(model.hasContextWindow && contextWindow !== undefined
      ? { contextWindow }
      : {}),
    ...(model.hasMaxTokens && maxTokens !== undefined ? { maxTokens } : {}),
    ...(model.hasThinkingLevelMap
      ? { thinkingLevelMap: model.thinkingLevelMap }
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
  compat,
  includeCompat,
  models,
  includeModels,
}: {
  passthrough: Record<string, unknown>;
  nativeName?: string;
  baseUrl: string;
  api: string;
  includeApi: boolean;
  apiKey: string;
  headers: Record<string, string>;
  compat: Record<string, unknown>;
  includeCompat: boolean;
  models: Record<string, unknown>[];
  includeModels: boolean;
}): Record<string, unknown> {
  return {
    ...passthrough,
    ...(nativeName !== undefined ? { name: nativeName } : {}),
    ...(baseUrl.trim() ? { baseUrl: baseUrl.trim() } : {}),
    ...(includeApi && api.trim() ? { api: api.trim() } : {}),
    ...(apiKey ? { apiKey } : {}),
    ...(Object.keys(headers).length > 0 ? { headers } : {}),
    ...(includeCompat ? { compat } : {}),
    ...(includeModels ? { models } : {}),
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
  const [nativeNameFollowsDisplay, setNativeNameFollowsDisplay] = useState(
    () =>
      !isEdit ||
      (initialConfigHasNativeName &&
        initialNativeName === initialDisplayName.trim()),
  );
  const [nativeNameOverride, setNativeNameOverride] = useState<
    string | undefined
  >(() => (initialConfigHasNativeName ? initialNativeName : undefined));
  const displayNameBaselineRef = useRef(initialDisplayName.trim());
  const resolveNativeName = useCallback(
    (displayName: string): string | undefined => {
      return nativeNameFollowsDisplay ? displayName.trim() : nativeNameOverride;
    },
    [nativeNameFollowsDisplay, nativeNameOverride],
  );
  const [selectedPresetId, setSelectedPresetId] = useState<string | null>(
    isEdit ? null : "custom",
  );
  const initialPreset = useMemo(() => {
    if (!providerId) return null;
    const preset =
      piProviderPresets.find(
        (candidate) => candidate.providerKey === providerId,
      ) ?? null;
    if (!preset || !isEdit) return preset;

    return optionalText(initialConfig.baseUrl) ===
      preset.settingsConfig.baseUrl &&
      optionalText(initialConfig.api) === preset.settingsConfig.api
      ? preset
      : null;
  }, [initialConfig.api, initialConfig.baseUrl, isEdit, providerId]);
  const [selectedPreset, setSelectedPreset] = useState<PiProviderPreset | null>(
    initialPreset,
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
  const includeModelsRef = useRef(!isEdit || hasOwn(initialConfig, "models"));
  const [apiKey, setApiKey] = useState(optionalText(initialConfig.apiKey));
  const initialHeaders = useMemo(
    () => asObject(initialConfig.headers),
    [initialConfig.headers],
  );
  const [providerHeaders, setProviderHeaders] = useState<
    Record<string, string>
  >(() => stringRecord(initialHeaders));
  const [providerCompat, setProviderCompat] = useState<Record<string, unknown>>(
    () => asObject(initialConfig.compat),
  );
  const includeCompatRef = useRef(hasOwn(initialConfig, "compat"));
  const [providerPassthrough, setProviderPassthrough] = useState<
    Record<string, unknown>
  >(() => objectWithout(initialConfig, ROOT_CONTROLLED_KEYS));
  const [fetchedModels, setFetchedModels] = useState<FetchedModel[]>([]);
  const [isFetchingModels, setIsFetchingModels] = useState(false);
  const modelFetchGenerationRef = useRef(0);
  const [formError, setFormError] = useState<string | null>(null);
  const initialModels = useMemo<PiModelDraft[]>(() => {
    const configured = Array.isArray(initialConfig.models)
      ? initialConfig.models
      : [];
    return configured.map((model) => modelDraft(model));
  }, [initialConfig.models]);
  const [models, setModelsState] = useState<PiModelDraft[]>(initialModels);
  const modelsRef = useRef(initialModels);
  const [expandedModelKeys, setExpandedModelKeys] = useState<Set<string>>(
    () => new Set(),
  );
  const [expandedThinkingMapKeys, setExpandedThinkingMapKeys] = useState<
    Set<string>
  >(() => new Set());
  const [editingThinkingLevel, setEditingThinkingLevel] = useState<{
    modelKey: string;
    level: PiThinkingLevel;
  } | null>(null);
  const initialSettingsConfigText = useMemo(
    () =>
      JSON.stringify(
        isEdit
          ? {
              ...initialConfig,
              ...(Array.isArray(initialConfig.models)
                ? { models: initialModels.map(modelPreview) }
                : {}),
            }
          : buildPiSettingsConfig({
              passthrough: {},
              nativeName: initialDisplayName.trim(),
              baseUrl: "",
              api: "openai-completions",
              includeApi: true,
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
    [initialConfig, initialDisplayName, initialModels, isEdit],
  );
  const identityDefaults = useMemo<ProviderFormData>(
    () => ({
      name: initialData?.name ?? optionalText(initialConfig.name),
      websiteUrl: initialData?.websiteUrl ?? "",
      notes: initialData?.notes ?? "",
      settingsConfig: initialSettingsConfigText,
      icon: initialData?.icon ?? "",
      iconColor: initialData?.iconColor ?? "",
    }),
    [initialConfig, initialData, initialSettingsConfigText],
  );
  const form = useForm<ProviderFormData>({
    resolver: zodResolver(providerSchema),
    defaultValues: identityDefaults,
    mode: "onSubmit",
  });
  const lastValidSettingsConfigRef = useRef<Record<string, unknown>>(
    parseJsonObject(initialSettingsConfigText) ?? {},
  );
  const settingsConfigText = form.watch("settingsConfig");
  const isSettingsConfigValid = parseJsonObject(settingsConfigText) !== null;
  const displayName = form.watch("name");
  const hasConfigurationSelection = isEdit || selectedPresetId !== null;
  const isSubmitReady = hasConfigurationSelection;

  const replaceModelsState = useCallback((nextModels: PiModelDraft[]) => {
    modelsRef.current = nextModels;
    setModelsState(nextModels);
  }, []);

  const invalidateFetchedModels = useCallback(() => {
    modelFetchGenerationRef.current += 1;
    setFetchedModels([]);
    setIsFetchingModels(false);
  }, []);

  useEffect(
    () => () => {
      modelFetchGenerationRef.current += 1;
    },
    [],
  );

  const updateSettingsConfig = useCallback(
    (update: (config: Record<string, unknown>) => void) => {
      const config = parseJsonObject(form.getValues("settingsConfig"));
      if (!config) return false;
      update(config);
      lastValidSettingsConfigRef.current = config;
      form.setValue("settingsConfig", JSON.stringify(config, null, 2), {
        shouldDirty: true,
        shouldValidate: true,
      });
      return true;
    },
    [form],
  );

  const syncModelsToSettingsConfig = useCallback(
    (nextModels: PiModelDraft[]) => {
      return updateSettingsConfig((config) => {
        if (includeModelsRef.current || nextModels.length > 0) {
          config.models = nextModels.map(modelPreview);
        } else {
          delete config.models;
        }
      });
    },
    [updateSettingsConfig],
  );

  const commitModels = useCallback(
    (nextModels: PiModelDraft[]) => {
      if (!syncModelsToSettingsConfig(nextModels)) return false;
      replaceModelsState(nextModels);
      return true;
    },
    [replaceModelsState, syncModelsToSettingsConfig],
  );

  const applySettingsConfig = useCallback(
    (
      config: Record<string, unknown>,
      previousConfig: Record<string, unknown> | null,
    ) => {
      const currentDisplayName = form.getValues("name").trim();
      const hasNativeName =
        hasOwn(config, "name") && typeof config.name === "string";
      const nextNativeName = hasNativeName
        ? (config.name as string)
        : undefined;

      setNativeNameOverride(nextNativeName);
      setNativeNameFollowsDisplay(
        hasNativeName && nextNativeName === currentDisplayName,
      );
      setBaseUrl(optionalText(config.baseUrl));
      const nextApi = optionalText(config.api) || "openai-completions";
      setApi(nextApi);
      setIncludeApi(hasOwn(config, "api"));
      setApiKey(optionalText(config.apiKey));
      setProviderHeaders(stringRecord(asObject(config.headers)));
      setProviderCompat(asObject(config.compat));
      includeCompatRef.current = hasOwn(config, "compat");
      setProviderPassthrough(objectWithout(config, ROOT_CONTROLLED_KEYS));
      includeModelsRef.current = hasOwn(config, "models");

      const requestConfigChanged =
        !previousConfig ||
        optionalText(previousConfig.baseUrl) !== optionalText(config.baseUrl) ||
        (optionalText(previousConfig.api) || "openai-completions") !==
          (optionalText(config.api) || "openai-completions") ||
        optionalText(previousConfig.apiKey) !== optionalText(config.apiKey) ||
        !jsonValuesEqual(
          stringRecord(asObject(previousConfig.headers)),
          stringRecord(asObject(config.headers)),
        );
      if (requestConfigChanged) invalidateFetchedModels();

      const previousModelValues =
        previousConfig && Array.isArray(previousConfig.models)
          ? previousConfig.models
          : [];
      const previousDrafts = modelsRef.current;
      const configModels = Array.isArray(config.models) ? config.models : [];
      const reservedPreviousIndexes = new Set<number>();
      const exactPreviousIndexes = configModels.map((model) => {
        const id = optionalText(asObject(model).id);
        const previousIndex = previousDrafts.findIndex(
          (candidate, candidateIndex) =>
            !reservedPreviousIndexes.has(candidateIndex) && candidate.id === id,
        );
        if (previousIndex >= 0) reservedPreviousIndexes.add(previousIndex);
        return previousIndex;
      });
      const usedPreviousIndexes = new Set(reservedPreviousIndexes);
      const canUsePositionalFallback =
        configModels.length === previousModelValues.length &&
        previousDrafts.length === previousModelValues.length;
      const nextModels = configModels.map((model, index) => {
        let previousIndex = exactPreviousIndexes[index];
        if (
          previousIndex < 0 &&
          canUsePositionalFallback &&
          previousDrafts[index] &&
          !usedPreviousIndexes.has(index)
        ) {
          previousIndex = index;
          usedPreviousIndexes.add(index);
        }

        const previousDraft =
          previousIndex >= 0 ? previousDrafts[previousIndex] : undefined;
        return modelDraft(model, { key: previousDraft?.key });
      });
      replaceModelsState(nextModels);
      const nextKeys = new Set(nextModels.map((model) => model.key));
      setExpandedModelKeys(
        (current) => new Set([...current].filter((key) => nextKeys.has(key))),
      );

      if (
        Array.isArray(config.models) &&
        !jsonValuesEqual(config.models, nextModels.map(modelPreview))
      ) {
        const normalizedConfig = {
          ...config,
          models: nextModels.map(modelPreview),
        };
        lastValidSettingsConfigRef.current = normalizedConfig;
        form.setValue(
          "settingsConfig",
          JSON.stringify(normalizedConfig, null, 2),
          {
            shouldDirty: true,
            shouldValidate: true,
          },
        );
      } else {
        lastValidSettingsConfigRef.current = config;
      }
    },
    [form, invalidateFetchedModels, replaceModelsState],
  );

  const handleSettingsConfigChange = useCallback(
    (value: string) => {
      // JsonEditor also emits when an external value is written into its
      // document. Structured controls already own that update, so applying it
      // again would recreate model row keys and collapse their details.
      if (value === form.getValues("settingsConfig")) return;

      const previousConfig =
        parseJsonObject(form.getValues("settingsConfig")) ??
        lastValidSettingsConfigRef.current;
      form.setValue("settingsConfig", value, {
        shouldDirty: true,
        shouldValidate: true,
      });

      const config = parseJsonObject(value);
      if (!config) {
        try {
          JSON.parse(value);
          form.setError("settingsConfig", {
            type: "validate",
            message: t("jsonEditor.mustBeObject"),
          });
        } catch {
          // providerSchema reports syntax errors without replacing the draft.
        }
        return;
      }

      form.clearErrors("settingsConfig");
      applySettingsConfig(config, previousConfig);
    },
    [applySettingsConfig, form, t],
  );

  useEffect(() => {
    const nextDisplayName = displayName.trim();
    if (displayNameBaselineRef.current === nextDisplayName) return;
    if (!nativeNameFollowsDisplay) {
      displayNameBaselineRef.current = nextDisplayName;
      return;
    }

    const applied = updateSettingsConfig((config) => {
      config.name = nextDisplayName;
    });
    if (!applied) {
      form.setValue("name", displayNameBaselineRef.current);
      return;
    }
    displayNameBaselineRef.current = nextDisplayName;
    setNativeNameOverride(nextDisplayName);
  }, [displayName, form, nativeNameFollowsDisplay, updateSettingsConfig]);

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
    invalidateFetchedModels();
    if (id === "custom") {
      setSelectedPreset(null);
      setCategory("custom");
      setProviderKey("");
      setNativeNameFollowsDisplay(true);
      setNativeNameOverride(identityDefaults.name.trim());
      displayNameBaselineRef.current = identityDefaults.name.trim();
      lastValidSettingsConfigRef.current =
        parseJsonObject(identityDefaults.settingsConfig) ?? {};
      form.reset(identityDefaults);
      setBaseUrl("");
      setApi("openai-completions");
      setIncludeApi(true);
      includeModelsRef.current = true;
      setApiKey("");
      setProviderHeaders({});
      setProviderCompat({});
      includeCompatRef.current = false;
      setProviderPassthrough({});
      replaceModelsState([]);
      setExpandedModelKeys(new Set());
      setExpandedThinkingMapKeys(new Set());
      return;
    }
    const entry = presetEntries.find((candidate) => candidate.id === id);
    if (!entry) return;
    const preset = entry.preset;
    const presetConfig = asObject(preset.settingsConfig);
    const nextModels = preset.settingsConfig.models.map((model) =>
      modelDraft(model),
    );
    setSelectedPreset(preset);
    setCategory(preset.category ?? "custom");
    setProviderKey(preset.providerKey);
    setNativeNameFollowsDisplay(true);
    setNativeNameOverride(preset.settingsConfig.name);
    displayNameBaselineRef.current = preset.settingsConfig.name.trim();
    lastValidSettingsConfigRef.current = presetConfig;
    form.reset({
      name: preset.settingsConfig.name,
      websiteUrl: preset.websiteUrl,
      notes: "",
      settingsConfig: JSON.stringify(presetConfig, null, 2),
      icon: preset.icon ?? "",
      iconColor: preset.iconColor ?? "",
    });
    setBaseUrl(preset.settingsConfig.baseUrl);
    setApi(preset.settingsConfig.api);
    setIncludeApi(true);
    includeModelsRef.current = true;
    setApiKey("");
    setProviderHeaders(stringRecord(asObject(presetConfig.headers)));
    setProviderCompat(asObject(presetConfig.compat));
    includeCompatRef.current = hasOwn(presetConfig, "compat");
    setProviderPassthrough(objectWithout(presetConfig, ROOT_CONTROLLED_KEYS));
    replaceModelsState(nextModels);
    setExpandedModelKeys(new Set());
    setExpandedThinkingMapKeys(new Set());
  };

  const updateModelOverride = (
    key: string,
    update: Partial<Omit<PiModelDraft, "key">>,
  ) => {
    commitModels(
      modelsRef.current.map((model) =>
        model.key === key ? { ...model, ...update } : model,
      ),
    );
  };

  const changeModelId = (key: string, id: string) => {
    commitModels(
      modelsRef.current.map((model) =>
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

  const updateThinkingLevelMap = (
    key: string,
    update: (map: PiThinkingLevelMap) => PiThinkingLevelMap,
  ) => {
    commitModels(
      modelsRef.current.map((model) => {
        if (model.key !== key) return model;
        const current = isPiThinkingLevelMap(model.thinkingLevelMap)
          ? { ...model.thinkingLevelMap }
          : {};
        return {
          ...model,
          thinkingLevelMap: update(current),
          hasThinkingLevelMap: true,
        };
      }),
    );
  };

  const updateThinkingLevelMode = (
    key: string,
    level: PiThinkingLevel,
    mode: PiThinkingLevelMode,
  ) => {
    updateThinkingLevelMap(key, (map) => {
      if (mode === "default") {
        delete map[level];
      } else if (mode === "unsupported") {
        map[level] = null;
      } else if (typeof map[level] !== "string") {
        map[level] = level;
      }
      return map;
    });
  };

  const addModel = () => {
    const model = newModel();
    const previouslyIncluded = includeModelsRef.current;
    includeModelsRef.current = true;
    if (!commitModels([...modelsRef.current, model])) {
      includeModelsRef.current = previouslyIncluded;
    }
  };

  const removeModel = (key: string) => {
    const nextModels = modelsRef.current.filter((model) => model.key !== key);
    commitModels(nextModels);
    setExpandedModelKeys((current) => {
      const next = new Set(current);
      next.delete(key);
      return next;
    });
    setExpandedThinkingMapKeys((current) => {
      const next = new Set(current);
      next.delete(key);
      return next;
    });
    setEditingThinkingLevel((current) =>
      current?.modelKey === key ? null : current,
    );
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
    setFetchedModels([]);
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

  const handleApiChange = useCallback(
    (value: string) => {
      const applied = updateSettingsConfig((config) => {
        config.api = value;
      });
      if (!applied) return;
      invalidateFetchedModels();
      setApi(value);
      setIncludeApi(true);
    },
    [invalidateFetchedModels, updateSettingsConfig],
  );

  const handleApiKeyChange = useCallback(
    (value: string) => {
      const applied = updateSettingsConfig((config) => {
        if (value) config.apiKey = value;
        else delete config.apiKey;
      });
      if (!applied) return;
      invalidateFetchedModels();
      setApiKey(value);
    },
    [invalidateFetchedModels, updateSettingsConfig],
  );

  const handleBaseUrlChange = useCallback(
    (value: string) => {
      const applied = updateSettingsConfig((config) => {
        const trimmed = value.trim();
        if (trimmed) config.baseUrl = trimmed;
        else delete config.baseUrl;
      });
      if (!applied) return;
      invalidateFetchedModels();
      setBaseUrl(value);
    },
    [invalidateFetchedModels, updateSettingsConfig],
  );

  const handleProviderHeadersChange = useCallback(
    (value: Record<string, string>) => {
      const applied = updateSettingsConfig((config) => {
        const normalized = normalizeRequestHeaders(value);
        if (Object.keys(normalized).length > 0) config.headers = normalized;
        else delete config.headers;
      });
      if (!applied) return;
      invalidateFetchedModels();
      setProviderHeaders(value);
    },
    [invalidateFetchedModels, updateSettingsConfig],
  );

  const handleProviderCompatChange = useCallback(
    (value: Record<string, unknown>) => {
      const includeCompat = Object.keys(value).length > 0;
      const applied = updateSettingsConfig((config) => {
        if (includeCompat) config.compat = value;
        else delete config.compat;
      });
      if (!applied) return;
      includeCompatRef.current = includeCompat;
      setProviderCompat(value);
    },
    [updateSettingsConfig],
  );

  const handleProviderKeyChange = useCallback((value: string) => {
    const normalized = value.toLowerCase().replace(/[^a-z0-9-]/g, "");
    setProviderKey(normalized);
  }, []);

  const submit = async (identity: ProviderFormData) => {
    onSubmittingChange?.(true);
    setFormError(null);
    try {
      if (!isEdit && selectedPresetId === null) {
        throw new PiFormValidationError(t("pi.form.selectPresetRequired"));
      }
      if (!parseJsonObject(identity.settingsConfig)) {
        throw new PiFormValidationError(
          t("jsonEditor.mustBeObject"),
          "#pi-settings-config",
        );
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
      if (!isEdit && selectedPreset && apiKey.length === 0) {
        throw new PiFormValidationError(
          t("pi.form.credentialRequired"),
          "#pi-api-key",
        );
      }
      if (!isEdit && models.length === 0) {
        throw new PiFormValidationError(
          t("pi.form.modelRequired"),
          "#pi-add-model",
          true,
        );
      }

      const headers = normalizeRequestHeaders(providerHeaders);
      const seen = new Set<string>();
      const normalizedModels = models.map((model, index) => {
        // Pi treats model IDs as opaque strings. Trimming would rename an
        // imported model.
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
        const displayName = model.name.trim();
        const includeName = !isEdit || model.hasName;
        const includeReasoning = !isEdit || model.hasReasoning;
        const includeInput = !isEdit || model.hasInput;
        const includeContextWindow = !isEdit || model.hasContextWindow;
        const includeMaxTokens = !isEdit || model.hasMaxTokens;
        if (includeName && !displayName) {
          throw new PiFormValidationError(
            t("pi.form.modelNameRequired", { index: index + 1 }),
            `#pi-model-name-${model.key}`,
            true,
          );
        }
        const contextWindow = includeContextWindow
          ? positiveNumber(
              model.contextWindow,
              t("pi.form.positiveNumberRequired", {
                label: t("pi.form.contextWindow"),
              }),
              `#pi-model-context-window-${model.key}`,
            )
          : undefined;
        const maxTokens = includeMaxTokens
          ? positiveNumber(
              model.maxTokens,
              t("pi.form.positiveNumberRequired", {
                label: t("pi.form.maxTokens"),
              }),
              `#pi-model-max-tokens-${model.key}`,
            )
          : undefined;
        if (
          model.hasThinkingLevelMap &&
          !isPiThinkingLevelMap(model.thinkingLevelMap)
        ) {
          throw new PiFormValidationError(
            t("pi.form.thinkingLevelMapInvalid"),
            `#pi-model-thinking-levels-${model.key}`,
            true,
          );
        }
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
        // Existing explicit nodes may be partial overrides of a Pi built-in
        // provider. Pi inherits the built-in transport in that case, so only
        // require a complete transport when CC Switch creates a new provider.
        if (!isEdit && !modelApi && !api.trim()) {
          throw new PiFormValidationError(
            t("pi.form.effectiveApiRequired", { id }),
            "#pi-provider-api-select",
            true,
          );
        }
        const effectiveUrl = modelBaseUrl || baseUrl.trim();
        if (!isEdit && !effectiveUrl) {
          throw new PiFormValidationError(
            t("pi.form.effectiveBaseUrlRequired", { id }),
            "#pi-provider-base-url",
          );
        }
        return {
          ...model.passthrough,
          id,
          ...(includeName ? { name: displayName } : {}),
          ...(includeReasoning ? { reasoning: model.reasoning } : {}),
          ...(includeInput
            ? {
                input: withImageInput(
                  model.input,
                  supportsImageInput(model.input),
                ),
              }
            : {}),
          ...(contextWindow !== undefined ? { contextWindow } : {}),
          ...(maxTokens !== undefined ? { maxTokens } : {}),
          ...(model.hasThinkingLevelMap
            ? { thinkingLevelMap: model.thinkingLevelMap }
            : {}),
        };
      });
      if (baseUrl.trim()) {
        validatePiField(
          () =>
            validateAbsoluteHttpUrl(
              baseUrl.trim(),
              t("pi.form.absoluteHttpUrlRequired", {
                label: t("opencode.baseUrl", { defaultValue: "Base URL" }),
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
        compat: providerCompat,
        includeCompat: includeCompatRef.current,
        models: normalizedModels,
        includeModels: includeModelsRef.current,
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
          /^#pi-model-(?:context-window|max-tokens|thinking-levels)-(.+)$/,
        );
        if (modelDetailsMatch) {
          setExpandedModelKeys((current) => {
            const next = new Set(current);
            next.add(modelDetailsMatch[1]);
            return next;
          });
          if (error.fieldSelector?.includes("thinking-levels")) {
            setExpandedThinkingMapKeys((current) => {
              const next = new Set(current);
              next.add(modelDetailsMatch[1]);
              return next;
            });
          }
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
            {t("pi.form.fixJsonFirst")}
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
                    <Label htmlFor="pi-provider-key">
                      {t("pi.form.providerKey")}
                      <span
                        aria-hidden="true"
                        className="text-destructive ml-1"
                      >
                        *
                      </span>
                    </Label>
                    <Input
                      id="pi-provider-key"
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
              htmlFor="pi-provider-api-select"
            >
              <Select value={api} onValueChange={handleApiChange}>
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
                {t("opencode.npmPackageHint", {
                  defaultValue: "选择 AI 服务的 API 接口格式",
                })}
              </p>
            </Field>

            <ApiKeySection
              id="pi-api-key"
              label={t("pi.form.credential")}
              value={apiKey}
              onChange={handleApiKeyChange}
              category={category}
              shouldShowLink={Boolean(selectedPreset?.apiKeyUrl)}
              websiteUrl={selectedPreset?.apiKeyUrl ?? ""}
              isPartner={selectedPreset?.isPartner}
              partnerPromotionKey={selectedPreset?.partnerPromotionKey}
            />

            <div className="space-y-2">
              <EndpointField
                id="pi-provider-base-url"
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
              id="pi-provider-compat"
              title={t("pi.form.compatibility")}
              hint={t("pi.form.compatibilityHint")}
              addLabel={t("pi.form.addCompatibilityOption")}
              emptyLabel={t("pi.form.noCompatibilityOptions")}
              keyLabel={t("pi.form.optionKey")}
              valueLabel={t("pi.form.optionValue")}
              keyPlaceholder="supportsDeveloperRole"
              valuePlaceholder="false"
              removeLabel={t("pi.form.removeCompatibilityOption")}
              options={providerCompat}
              onOptionsChange={handleProviderCompatChange}
            />

            <div
              id="pi-models-section"
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
                    <span className="flex-1">
                      {t("pi.form.modelId")}
                      <span
                        aria-hidden="true"
                        className="ml-1 text-destructive"
                      >
                        *
                      </span>
                    </span>
                    <span className="flex-1">
                      {t("pi.form.modelName")}
                      <span
                        aria-hidden="true"
                        className="ml-1 text-destructive"
                      >
                        *
                      </span>
                    </span>
                    <span className="w-9" />
                  </div>
                  {models.map((model) => {
                    const isExpanded = expandedModelKeys.has(model.key);
                    const validThinkingLevelMap = isPiThinkingLevelMap(
                      model.thinkingLevelMap,
                    )
                      ? model.thinkingLevelMap
                      : undefined;
                    const editableThinkingLevelMap =
                      validThinkingLevelMap ??
                      (!model.hasThinkingLevelMap ? {} : undefined);
                    const thinkingMapIsExpanded = expandedThinkingMapKeys.has(
                      model.key,
                    );
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
                                isExpanded ? "rotate-90" : ""
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
                              required
                              className="min-w-0 flex-1"
                            />
                            {fetchedModels.length > 0 && (
                              <ModelDropdown
                                models={fetchedModels}
                                onSelect={(id) => changeModelId(model.key, id)}
                              />
                            )}
                          </div>
                          <Input
                            id={`pi-model-name-${model.key}`}
                            value={model.name}
                            onChange={(event) =>
                              updateModelOverride(model.key, {
                                name: event.target.value,
                                hasName: true,
                              })
                            }
                            placeholder={t("pi.form.modelNamePlaceholder")}
                            aria-label={t("pi.form.modelName")}
                            required={!isEdit || model.hasName}
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

                        {isExpanded && (
                          <div className="ml-9 grid gap-3 border-l-2 border-muted pl-4 sm:grid-cols-2">
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
                                    updateModelOverride(model.key, {
                                      reasoning: checked,
                                      hasReasoning: true,
                                    })
                                  }
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
                              label={
                                <>
                                  {t("pi.form.contextWindow")}
                                  <span
                                    aria-hidden="true"
                                    className="ml-1 text-destructive"
                                  >
                                    *
                                  </span>
                                </>
                              }
                              htmlFor={`pi-model-context-window-${model.key}`}
                            >
                              <Input
                                id={`pi-model-context-window-${model.key}`}
                                aria-label={t("pi.form.contextWindow")}
                                type="number"
                                step="any"
                                min="1"
                                inputMode="decimal"
                                required={!isEdit || model.hasContextWindow}
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
                              label={
                                <>
                                  {t("pi.form.maxTokens")}
                                  <span
                                    aria-hidden="true"
                                    className="ml-1 text-destructive"
                                  >
                                    *
                                  </span>
                                </>
                              }
                              htmlFor={`pi-model-max-tokens-${model.key}`}
                            >
                              <Input
                                id={`pi-model-max-tokens-${model.key}`}
                                aria-label={t("pi.form.maxTokens")}
                                type="number"
                                step="any"
                                min="1"
                                inputMode="decimal"
                                required={!isEdit || model.hasMaxTokens}
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
                            {model.reasoning === true && (
                              <div
                                id={`pi-model-thinking-levels-${model.key}`}
                                tabIndex={-1}
                                className="w-full space-y-2 sm:col-span-2"
                              >
                                <div className="flex min-h-9 items-center">
                                  <Button
                                    type="button"
                                    variant="ghost"
                                    size="sm"
                                    onClick={() =>
                                      setExpandedThinkingMapKeys((current) => {
                                        const next = new Set(current);
                                        if (next.has(model.key)) {
                                          next.delete(model.key);
                                          setEditingThinkingLevel((editing) =>
                                            editing?.modelKey === model.key
                                              ? null
                                              : editing,
                                          );
                                        } else {
                                          next.add(model.key);
                                        }
                                        return next;
                                      })
                                    }
                                    aria-label={
                                      thinkingMapIsExpanded
                                        ? t("common.collapse")
                                        : t("pi.form.customizeThinkingLevels")
                                    }
                                    aria-expanded={thinkingMapIsExpanded}
                                    className="-ml-2 h-8 gap-1.5 px-2 text-foreground"
                                  >
                                    <span>
                                      {t("pi.form.thinkingLevelsLabel")}
                                    </span>
                                    <ChevronDown
                                      className={`h-4 w-4 transition-transform motion-reduce:transition-none ${
                                        thinkingMapIsExpanded
                                          ? "rotate-180"
                                          : ""
                                      }`}
                                    />
                                  </Button>
                                </div>

                                {thinkingMapIsExpanded &&
                                  editableThinkingLevelMap && (
                                    <div className="overflow-hidden rounded-lg border border-border/70 bg-background/30">
                                      {PI_THINKING_LEVELS.map((level) => {
                                        const mode = thinkingLevelMode(
                                          editableThinkingLevelMap,
                                          level,
                                        );
                                        const mappedValue =
                                          editableThinkingLevelMap[level];
                                        const popoverOpen =
                                          editingThinkingLevel?.modelKey ===
                                            model.key &&
                                          editingThinkingLevel.level === level;
                                        return (
                                          <Popover
                                            key={level}
                                            open={popoverOpen}
                                            onOpenChange={(open) =>
                                              setEditingThinkingLevel(
                                                open
                                                  ? {
                                                      modelKey: model.key,
                                                      level,
                                                    }
                                                  : null,
                                              )
                                            }
                                          >
                                            <PopoverTrigger asChild>
                                              <button
                                                type="button"
                                                aria-label={t(
                                                  "pi.form.editThinkingLevel",
                                                  {
                                                    level: t(
                                                      `pi.form.thinkingLevels.${level}`,
                                                    ),
                                                  },
                                                )}
                                                className="group flex h-[42px] w-full items-center gap-3 border-b border-border/40 px-4 text-left text-sm transition-colors last:border-b-0 hover:bg-muted/40"
                                              >
                                                <span className="flex-1">
                                                  {t(
                                                    `pi.form.thinkingLevels.${level}`,
                                                  )}
                                                </span>
                                                <span
                                                  className={
                                                    mode === "value"
                                                      ? "max-w-[18rem] truncate text-right font-mono text-xs text-foreground"
                                                      : "text-xs text-muted-foreground"
                                                  }
                                                >
                                                  {mode === "default"
                                                    ? t(
                                                        "pi.form.thinkingLevelDefault",
                                                      )
                                                    : mode === "unsupported"
                                                      ? t(
                                                          "pi.form.thinkingLevelUnsupported",
                                                        )
                                                      : mappedValue}
                                                </span>
                                                <PopoverAnchor asChild>
                                                  <span
                                                    className={`inline-flex h-6 w-6 shrink-0 items-center justify-center rounded-md transition-[color,background-color,transform] duration-200 ${
                                                      popoverOpen
                                                        ? "translate-x-0.5 bg-primary/10 text-primary"
                                                        : "text-muted-foreground/50 group-hover:translate-x-0.5 group-hover:text-muted-foreground"
                                                    }`}
                                                  >
                                                    <ChevronRight className="h-3.5 w-3.5" />
                                                  </span>
                                                </PopoverAnchor>
                                              </button>
                                            </PopoverTrigger>
                                            <PopoverContent
                                              side="left"
                                              align="center"
                                              sideOffset={10}
                                              collisionPadding={24}
                                              sticky="always"
                                              className="pi-thinking-popover z-[1000] w-72 space-y-3 p-4 shadow-xl"
                                            >
                                              <p className="text-sm font-medium">
                                                {t(
                                                  `pi.form.thinkingLevels.${level}`,
                                                )}
                                              </p>
                                              <label className="flex cursor-pointer items-center gap-2.5 text-sm">
                                                <input
                                                  type="radio"
                                                  name={`pi-thinking-level-mode-${model.key}-${level}`}
                                                  checked={mode === "default"}
                                                  onChange={() =>
                                                    updateThinkingLevelMode(
                                                      model.key,
                                                      level,
                                                      "default",
                                                    )
                                                  }
                                                  className="h-4 w-4 accent-primary"
                                                />
                                                {t(
                                                  "pi.form.thinkingLevelFollowDefault",
                                                )}
                                              </label>
                                              <div className="flex items-center gap-2.5 text-sm">
                                                <input
                                                  id={`pi-thinking-level-value-${model.key}-${level}`}
                                                  type="radio"
                                                  name={`pi-thinking-level-mode-${model.key}-${level}`}
                                                  checked={mode === "value"}
                                                  aria-label={t(
                                                    "pi.form.thinkingLevelMapTo",
                                                  )}
                                                  onChange={() =>
                                                    updateThinkingLevelMode(
                                                      model.key,
                                                      level,
                                                      "value",
                                                    )
                                                  }
                                                  className="h-4 w-4 accent-primary"
                                                />
                                                <label
                                                  htmlFor={`pi-thinking-level-value-${model.key}-${level}`}
                                                  className="shrink-0 cursor-pointer"
                                                >
                                                  {t(
                                                    "pi.form.thinkingLevelMapTo",
                                                  )}
                                                </label>
                                                <Input
                                                  value={
                                                    typeof mappedValue ===
                                                    "string"
                                                      ? mappedValue
                                                      : level
                                                  }
                                                  onChange={(event) =>
                                                    updateThinkingLevelMap(
                                                      model.key,
                                                      (map) => ({
                                                        ...map,
                                                        [level]:
                                                          event.target.value,
                                                      }),
                                                    )
                                                  }
                                                  onFocus={() =>
                                                    mode !== "value" &&
                                                    updateThinkingLevelMode(
                                                      model.key,
                                                      level,
                                                      "value",
                                                    )
                                                  }
                                                  aria-label={t(
                                                    "pi.form.thinkingLevelValue",
                                                    {
                                                      level: t(
                                                        `pi.form.thinkingLevels.${level}`,
                                                      ),
                                                    },
                                                  )}
                                                  className="h-8 min-w-0 flex-1 font-mono text-xs"
                                                />
                                              </div>
                                              <label className="flex cursor-pointer items-center gap-2.5 text-sm">
                                                <input
                                                  type="radio"
                                                  name={`pi-thinking-level-mode-${model.key}-${level}`}
                                                  checked={
                                                    mode === "unsupported"
                                                  }
                                                  onChange={() =>
                                                    updateThinkingLevelMode(
                                                      model.key,
                                                      level,
                                                      "unsupported",
                                                    )
                                                  }
                                                  className="h-4 w-4 accent-primary"
                                                />
                                                {t(
                                                  "pi.form.thinkingLevelMarkUnavailable",
                                                )}
                                              </label>
                                            </PopoverContent>
                                          </Popover>
                                        );
                                      })}
                                    </div>
                                  )}
                              </div>
                            )}
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
          <FormField
            control={form.control}
            name="settingsConfig"
            render={() => (
              <FormItem className="space-y-2">
                <Label htmlFor="pi-settings-config">
                  {t("provider.configJson")}
                </Label>
                <JsonEditor
                  id="pi-settings-config"
                  ariaLabel={t("provider.configJson")}
                  value={settingsConfigText}
                  onChange={handleSettingsConfigChange}
                  height={
                    Math.max(1, settingsConfigText.split("\n").length) * 20 + 20
                  }
                  showValidation={true}
                  language="json"
                  darkMode={isDarkMode}
                />
                <FormMessage />
              </FormItem>
            )}
          />
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
