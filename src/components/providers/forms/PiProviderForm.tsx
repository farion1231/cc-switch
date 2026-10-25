import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { ChevronDown, ChevronRight, Plus, Trash2 } from "lucide-react";
import { useForm } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import { toast } from "@/lib/toast";
import { Button } from "@/components/ui/button";
import { HoverTip } from "@/components/ui/hover-tip";
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
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
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
import { BasicFormFields, REQUIRED_LABEL } from "./BasicFormFields";
import { ProviderPresetSelector } from "./ProviderPresetSelector";
import { useProvidersQuery } from "@/lib/query/queries";
import { cn } from "@/lib/utils";
import {
  FetchedModelPicker,
  type FetchedModelSelection,
} from "./FetchedModelPicker";
import { RequestHeadersEditor } from "./RequestHeadersEditor";
import { StructuredOptionsEditor } from "./StructuredOptionsEditor";
import {
  ApiKeySection,
  EndpointField,
  GptContextWindowHint,
  ModelDropdown,
} from "./shared";
import {
  findRequestHeaderValue,
  normalizeRequestHeaders,
} from "./helpers/requestHeaders";
import {
  ompProviderPresets,
  type OmpProviderPreset,
} from "@/config/ompProviderPresets";
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
import { useLatestRef } from "@/hooks/useLatestRef";
import { useModelMetadataFill } from "@/hooks/useModelMetadataFill";
import type { KnownModelMetadata } from "@/lib/modelMetadata";
import { piPresetModelSources } from "@/config/presetModelMetadata";
import {
  metadataFilledAnything,
  type PiProviderProtocol,
  piPresetThinkingFor,
  piSendsReasoningEffort,
  piThinkingLevelMapFromEfforts,
} from "./modelMetadataFill";
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
const OMP_ROOT_CONTROLLED_KEYS = new Set(
  [...ROOT_CONTROLLED_KEYS].filter((key) => key !== "name"),
);
// omp 的 `discovery.type`（docs/models.md）：运行时从端点拉模型列表，可以不写 models。
// proxy 每个模型自己判断接口格式，其余类型要求供应商级 api（后端校验）。
// 值里有点号，不能直接当 i18n key，所以另配一个 key。
const OMP_DISCOVERY_TYPES: ReadonlyArray<{ value: string; labelKey: string }> =
  [
    { value: "openai-models-list", labelKey: "openaiModelsList" },
    { value: "proxy", labelKey: "proxy" },
    { value: "ollama", labelKey: "ollama" },
    { value: "llama.cpp", labelKey: "llamaCpp" },
    { value: "lm-studio", labelKey: "lmStudio" },
    { value: "litellm", labelKey: "litellm" },
    { value: "apple-foundation-models", labelKey: "appleFoundationModels" },
  ];
const OMP_DISCOVERY_OFF = "__off__";
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

/** 这一行实际使用的协议：模型级的 `api`、`baseUrl`、`compat` 覆盖供应商级。 */
function piModelProtocol(
  model: PiModelDraft,
  provider: PiProviderProtocol,
): PiProviderProtocol {
  const { passthrough } = model;
  return {
    ...provider,
    api: optionalText(passthrough.api) || provider.api,
    baseUrl: optionalText(passthrough.baseUrl) || provider.baseUrl,
    compat: { ...provider.compat, ...asObject(passthrough.compat) },
  };
}

/**
 * 选中拉取到的模型后补上已知参数：数字只补空的，推理和图片输入只往「支持」补。
 * `thinkingLevelMap` 只在模型还没有映射且支持推理时补：同地址 Pi 预设核对过的
 * 映射优先，但只在协议相同、且预设依赖的 compat 不与用户已写的值冲突时采用，
 * 缺的 compat 一并补进模型；否则仅当 Pi 会把档位原样作为 `reasoning_effort` 发出
 * 时，按 Pi 官方规则从 effort 档位生成。
 */
function fillPiModelDraft(
  model: PiModelDraft,
  metadata: KnownModelMetadata,
  provider: PiProviderProtocol,
  allowPiThinking = true,
): PiModelDraft {
  const next = { ...model };
  if (!model.contextWindow.trim() && metadata.contextWindow) {
    next.contextWindow = String(metadata.contextWindow);
    next.hasContextWindow = true;
  }
  if (!model.maxTokens.trim() && metadata.maxOutputTokens) {
    next.maxTokens = String(metadata.maxOutputTokens);
    next.hasMaxTokens = true;
  }
  if (metadata.reasoning === true && !model.reasoning) {
    next.reasoning = true;
    next.hasReasoning = true;
  }
  if (
    metadata.inputModalities?.includes("image") &&
    !supportsImageInput(model.input)
  ) {
    next.input = withImageInput(model.input, true);
    next.hasInput = true;
  }
  if (allowPiThinking && !model.hasThinkingLevelMap && next.reasoning) {
    const effective = piModelProtocol(model, provider);
    const preset = piPresetThinkingFor(metadata.piThinking, effective);
    const map =
      preset?.map ??
      (piSendsReasoningEffort(effective)
        ? piThinkingLevelMapFromEfforts(metadata.reasoningEfforts)
        : undefined);
    if (map) {
      next.thinkingLevelMap = map;
      next.hasThinkingLevelMap = true;
      if (preset && Object.keys(preset.missingCompat).length > 0) {
        next.passthrough = {
          ...model.passthrough,
          compat: {
            ...asObject(model.passthrough.compat),
            ...preset.missingCompat,
          },
        };
      }
    }
  }
  return next;
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

function piModelPreview(model: PiModelDraft): Record<string, unknown> {
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
  appId,
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
  const isOmp = appId === "omp";
  const rootControlledKeys = isOmp
    ? OMP_ROOT_CONTROLLED_KEYS
    : ROOT_CONTROLLED_KEYS;
  const presets = isOmp ? ompProviderPresets : piProviderPresets;
  const modelPreview = (model: PiModelDraft) => {
    const value = piModelPreview(model);
    if (isOmp) delete value.thinkingLevelMap;
    return value;
  };
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
      !isOmp &&
      (!isEdit ||
        (initialConfigHasNativeName &&
          initialNativeName === initialDisplayName.trim())),
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
      presets.find((candidate) => candidate.providerKey === providerId) ?? null;
    if (!preset || !isEdit) return preset;

    return optionalText(initialConfig.baseUrl) ===
      preset.settingsConfig.baseUrl &&
      optionalText(initialConfig.api) === preset.settingsConfig.api
      ? preset
      : null;
  }, [initialConfig.api, initialConfig.baseUrl, isEdit, providerId]);
  const [selectedPreset, setSelectedPreset] = useState<
    PiProviderPreset | OmpProviderPreset | null
  >(initialPreset);
  const [category, setCategory] = useState<ProviderCategory>(
    initialData?.category ?? "custom",
  );
  const [providerKey, setProviderKey] = useState(providerId ?? "");
  // Pi 和 omp 的供应商各管各的，查重只看当前应用。
  const { data: existingProviders } = useProvidersQuery(isOmp ? "omp" : "pi");
  // 预设自带标识；同一家加第二次时会撞上已有的卡，在字段下直接提示。
  const isDuplicateProviderKey =
    !isEdit &&
    Object.prototype.hasOwnProperty.call(
      existingProviders?.providers ?? {},
      providerKey.trim(),
    );
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
  >(() => objectWithout(initialConfig, rootControlledKeys));
  const [fetchedModels, setFetchedModels] = useState<FetchedModel[]>([]);
  const [isFetchingModels, setIsFetchingModels] = useState(false);
  // 请求头、自动发现和兼容性很少需要动，默认收起；预设填了值也不自动展开。
  const [advancedExpanded, setAdvancedExpanded] = useState(false);
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
              nativeName: isOmp ? undefined : initialDisplayName.trim(),
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
        !isOmp && hasNativeName && nextNativeName === currentDisplayName,
      );
      setBaseUrl(optionalText(config.baseUrl));
      const nextApi = optionalText(config.api) || "openai-completions";
      setApi(nextApi);
      setIncludeApi(hasOwn(config, "api"));
      setApiKey(optionalText(config.apiKey));
      setProviderHeaders(stringRecord(asObject(config.headers)));
      setProviderCompat(asObject(config.compat));
      includeCompatRef.current = hasOwn(config, "compat");
      setProviderPassthrough(objectWithout(config, rootControlledKeys));
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
    [
      form,
      invalidateFetchedModels,
      isOmp,
      replaceModelsState,
      rootControlledKeys,
    ],
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
      presets.map((preset, index) => ({
        id: `pi-${index}`,
        preset,
      })),
    [presets],
  );

  const selectPreset = (id: string) => {
    setFormError(null);
    setSelectedPresetId(id);
    invalidateFetchedModels();
    if (id === "custom") {
      setSelectedPreset(null);
      setCategory("custom");
      setProviderKey("");
      setNativeNameFollowsDisplay(!isOmp);
      setNativeNameOverride(isOmp ? undefined : identityDefaults.name.trim());
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
    const presetName =
      optionalText(asObject(preset.settingsConfig).name) || preset.name;
    setNativeNameFollowsDisplay(!isOmp);
    setNativeNameOverride(isOmp ? undefined : presetName);
    displayNameBaselineRef.current = presetName.trim();
    lastValidSettingsConfigRef.current = presetConfig;
    form.reset({
      name: presetName,
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
    setProviderPassthrough(objectWithout(presetConfig, rootControlledKeys));
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

  const commitModelsRef = useLatestRef(commitModels);
  const protocolRef = useLatestRef<PiProviderProtocol>({
    api,
    baseUrl,
    providerId: providerKey,
    compat: providerCompat,
  });
  const fillModelMetadata = useModelMetadataFill({
    baseUrl,
    presets: piPresetModelSources,
    prefetch: fetchedModels.length > 0,
  });

  // 补上模型已知的窗口、输出上限、推理、图片输入和思考档位。补全可能晚到：
  // 读最新的模型行，确认这一行仍是这个模型再补。
  const fillModelMetadataFor = (key: string, id: string, silent?: boolean) => {
    const row = modelsRef.current.find((model) => model.key === key);
    // 模型单独指向别的地址时，按它实际请求的地址查参数。
    const rowBaseUrl = row && piModelProtocol(row, protocolRef.current).baseUrl;
    fillModelMetadata(
      id,
      (metadata) => {
        const current = modelsRef.current.find((model) => model.key === key);
        if (current?.id !== id) return false;
        const filled = fillPiModelDraft(
          current,
          metadata,
          protocolRef.current,
          !isOmp,
        );
        if (!metadataFilledAnything(current, filled)) return false;
        return commitModelsRef.current(
          modelsRef.current.map((model) =>
            model.key === key ? filled : model,
          ),
        );
      },
      { baseUrl: rowBaseUrl, silent },
    );
  };

  const selectFetchedModelId = (key: string, id: string) => {
    changeModelId(key, id);
    fillModelMetadataFor(key, id);
  };

  // 弹窗里勾选：去掉取消勾选的模型，追加新勾选的模型（已有同 ID 的行不重复加）。
  const handleApplyFetchedModels = ({ add, remove }: FetchedModelSelection) => {
    const removed = new Set(remove);
    const removedKeys = new Set(
      modelsRef.current
        .filter((model) => removed.has(model.id))
        .map((model) => model.key),
    );
    const kept = modelsRef.current.filter(
      (model) => !removedKeys.has(model.key),
    );
    const configured = new Set(kept.map((model) => model.id));
    const added = add
      .filter((id) => !configured.has(id))
      .map((id) => ({ ...newModel(), id, name: id }));
    const previouslyIncluded = includeModelsRef.current;
    if (added.length > 0) includeModelsRef.current = true;
    if (!commitModels([...kept, ...added])) {
      includeModelsRef.current = previouslyIncluded;
      return;
    }
    if (removedKeys.size > 0) {
      const prune = (current: Set<string>) =>
        new Set([...current].filter((key) => !removedKeys.has(key)));
      setExpandedModelKeys(prune);
      setExpandedThinkingMapKeys(prune);
      setEditingThinkingLevel((current) =>
        current && removedKeys.has(current.modelKey) ? null : current,
      );
    }
    // 弹窗里已经显示过这些参数，补完不再提示。
    for (const model of added) fillModelMetadataFor(model.key, model.id, true);
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
        const models = [
          ...new Map(result.map((model) => [model.id, model])).values(),
        ];
        setFetchedModels(models);
        // 拉到模型时直接打开选择弹窗，不再提示成功。
        if (models.length === 0) {
          toast.info(t("providerForm.fetchModelsEmpty"));
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

  // discovery 不受表单控制（在 passthrough 里），这里只改它的 type，其余子键原样保留。
  // 例外是 injectV1：omp 只允许它出现在 openai-models-list 上，留着会让整份 models.yml 失效。
  const ompDiscoveryType = optionalText(
    asObject(providerPassthrough.discovery).type,
  );
  const handleOmpDiscoveryChange = useCallback(
    (value: string) => {
      // 下拉在 <form> 里随折叠区挂载，Radix 可能回报一次空串，不能当成「关闭」
      if (!value) return;
      const next = { ...providerPassthrough };
      const { type: _previous, injectV1, ...extra } = asObject(next.discovery);
      if (value === "openai-models-list" && injectV1 !== undefined) {
        extra.injectV1 = injectV1;
      }
      if (value === OMP_DISCOVERY_OFF) delete next.discovery;
      else next.discovery = { type: value, ...extra };
      const applied = updateSettingsConfig((config) => {
        if (next.discovery) config.discovery = next.discovery;
        else delete config.discovery;
      });
      if (applied) setProviderPassthrough(next);
    },
    [providerPassthrough, updateSettingsConfig],
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
      if (isDuplicateProviderKey) {
        throw new PiFormValidationError(
          t("pi.form.providerKeyDuplicate"),
          "#pi-provider-key",
        );
      }
      if (!isEdit && selectedPreset && apiKey.length === 0) {
        throw new PiFormValidationError(
          t("pi.form.credentialRequired"),
          "#pi-api-key",
        );
      }
      if (!isEdit && models.length === 0 && !(isOmp && ompDiscoveryType)) {
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
          !isOmp &&
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
          ...(!isOmp && model.hasThinkingLevelMap
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
        nativeName: isOmp ? undefined : resolveNativeName(trimmedName),
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
        icon: identity.icon || selectedPreset?.icon || "",
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
        className="space-y-6"
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
            className="rounded-lg border border-transparent bg-warning-soft px-4 py-3 text-sm text-warning-text"
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
                <div className="space-y-2">
                  <Label htmlFor="pi-provider-key" className={REQUIRED_LABEL}>
                    {t("pi.form.providerKey")}
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
                  <p
                    className={
                      isDuplicateProviderKey
                        ? "text-xs text-destructive"
                        : "text-xs text-fg-2"
                    }
                  >
                    {isDuplicateProviderKey
                      ? t("pi.form.providerKeyDuplicate")
                      : isEdit
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
            </div>

            <div
              id="pi-models-section"
              tabIndex={-1}
              className="space-y-3 border-l border-border pl-3 outline-none"
            >
              <div className="flex items-center justify-between gap-3">
                <FormLabel>
                  {t("opencode.models", { defaultValue: "模型配置" })}
                </FormLabel>
                <div className="flex gap-1">
                  <FetchedModelPicker
                    models={fetchedModels}
                    configuredModelIds={models.map((model) => model.id)}
                    onApply={handleApplyFetchedModels}
                    onFetch={handleFetchModels}
                    isFetching={isFetchingModels}
                    metadataSource={{ baseUrl, presets: piPresetModelSources }}
                  />
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
                <p role="status" className="py-2 text-sm text-fg-2">
                  {t("pi.form.noModels", {
                    defaultValue: "暂无模型配置",
                  })}
                </p>
              ) : (
                <div className="space-y-2">
                  <div className="flex items-center gap-2 px-1 text-xs text-fg-2">
                    <span className="w-9" />
                    <span className={cn("flex-1", REQUIRED_LABEL)}>
                      {t("pi.form.modelId")}
                    </span>
                    <span className={cn("flex-1", REQUIRED_LABEL)}>
                      {t("pi.form.modelName")}
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
                                onSelect={(id) =>
                                  selectFetchedModelId(model.key, id)
                                }
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
                          <HoverTip content={t("pi.form.removeModel")}>
                            <Button
                              type="button"
                              variant="ghost"
                              size="icon"
                              onClick={() => removeModel(model.key)}
                              aria-label={t("pi.form.removeModel")}
                              className="h-9 w-9 shrink-0 text-fg-2 hover:text-destructive"
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
                              label={t("pi.form.contextWindow")}
                              required
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
                              <GptContextWindowHint
                                modelId={model.id}
                                contextWindow={model.contextWindow}
                              />
                            </Field>
                            <Field
                              label={t("pi.form.maxTokens")}
                              required
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
                            {!isOmp && model.reasoning === true && (
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
                                    className="-ml-2 h-8 gap-1.5 px-2 text-fg-1"
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
                                    <div className="overflow-hidden rounded-lg border border-border/70 bg-surface">
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
                                                className="group flex h-[42px] w-full items-center gap-3 border-b border-border/40 px-4 text-left text-sm transition-colors last:border-b-0 hover:bg-subtle"
                                              >
                                                <span className="flex-1">
                                                  {t(
                                                    `pi.form.thinkingLevels.${level}`,
                                                  )}
                                                </span>
                                                <span
                                                  className={
                                                    mode === "value"
                                                      ? "max-w-[18rem] truncate text-right font-mono text-xs text-fg-1"
                                                      : "text-xs text-fg-2"
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
                                                        : "text-fg-3 group-hover:translate-x-0.5 group-hover:text-fg-2"
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
                                                  className="ui-radio"
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
                                                  className="ui-radio"
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
                                                  className="ui-radio"
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
            </div>

            <Collapsible
              open={advancedExpanded}
              onOpenChange={setAdvancedExpanded}
              className="rounded-lg border border-border p-4"
            >
              <CollapsibleTrigger asChild>
                <Button
                  type="button"
                  variant={null}
                  size="sm"
                  className="h-8 w-full justify-start gap-1.5 px-0 text-sm font-medium text-fg-1 hover:opacity-70 active:scale-100"
                >
                  {advancedExpanded ? (
                    <ChevronDown className="h-4 w-4" />
                  ) : (
                    <ChevronRight className="h-4 w-4" />
                  )}
                  {t("providerForm.advancedOptionsToggle")}
                </Button>
              </CollapsibleTrigger>
              <CollapsibleContent className="space-y-6 pt-3">
                <RequestHeadersEditor
                  headers={providerHeaders}
                  onHeadersChange={handleProviderHeadersChange}
                />

                {isOmp && (
                  <Field
                    label={t("omp.form.discoveryType")}
                    htmlFor="omp-discovery-type"
                  >
                    <Select
                      value={ompDiscoveryType || OMP_DISCOVERY_OFF}
                      onValueChange={handleOmpDiscoveryChange}
                    >
                      <SelectTrigger id="omp-discovery-type" className="w-full">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value={OMP_DISCOVERY_OFF}>
                          {t("omp.form.discoveryOff")}
                        </SelectItem>
                        {OMP_DISCOVERY_TYPES.map(({ value, labelKey }) => (
                          <SelectItem key={value} value={value}>
                            {t(`omp.form.discoveryTypes.${labelKey}`)}
                          </SelectItem>
                        ))}
                        {ompDiscoveryType &&
                          !OMP_DISCOVERY_TYPES.some(
                            ({ value }) => value === ompDiscoveryType,
                          ) && (
                            <SelectItem value={ompDiscoveryType}>
                              {ompDiscoveryType}
                            </SelectItem>
                          )}
                      </SelectContent>
                    </Select>
                    <p className="text-xs text-muted-foreground">
                      {t("omp.form.discoveryHint")}
                    </p>
                  </Field>
                )}

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
              </CollapsibleContent>
            </Collapsible>
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
  required,
  children,
}: {
  label: React.ReactNode;
  htmlFor: string;
  required?: boolean;
  children: React.ReactNode;
}) {
  return (
    <div className="space-y-2">
      <Label
        htmlFor={htmlFor}
        className={required ? REQUIRED_LABEL : undefined}
      >
        {label}
      </Label>
      {children}
    </div>
  );
}
