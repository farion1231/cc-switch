import { useEffect, useMemo, useState, useCallback, useRef } from "react";
import { useForm } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Form, FormField, FormItem, FormMessage } from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import { providerSchema, type ProviderFormData } from "@/lib/schemas/provider";
import { providersApi, type AppId } from "@/lib/api";
import type {
  ProviderCategory,
  ProviderMeta,
  ProviderTestConfig,
  ProviderProxyConfig,
  ClaudeApiFormat,
  OpenCodeModel,
  OpenCodeProviderConfig,
  OpenClawModel,
} from "@/types";
import {
  providerPresets,
  type ProviderPreset,
} from "@/config/claudeProviderPresets";
import {
  codexProviderPresets,
  type CodexProviderPreset,
} from "@/config/codexProviderPresets";
import {
  geminiProviderPresets,
  type GeminiProviderPreset,
} from "@/config/geminiProviderPresets";
import {
  opencodeProviderPresets,
  OPENCODE_PRESET_MODEL_VARIANTS,
  type OpenCodeProviderPreset,
} from "@/config/opencodeProviderPresets";
import {
  openclawProviderPresets,
  type OpenClawProviderPreset,
  type OpenClawSuggestedDefaults,
} from "@/config/openclawProviderPresets";
import { OpenCodeFormFields } from "./OpenCodeFormFields";
import { OpenClawFormFields } from "./OpenClawFormFields";
import type { UniversalProviderPreset } from "@/config/universalProviderPresets";
import { applyTemplateValues } from "@/utils/providerConfigUtils";
import { mergeProviderMeta } from "@/utils/providerMetaUtils";
import { getCodexCustomTemplate } from "@/config/codexTemplates";
import CodexConfigEditor from "./CodexConfigEditor";
import { CommonConfigEditor } from "./CommonConfigEditor";
import GeminiConfigEditor from "./GeminiConfigEditor";
import JsonEditor from "@/components/JsonEditor";
import { Label } from "@/components/ui/label";
import { ProviderPresetSelector } from "./ProviderPresetSelector";
import { BasicFormFields } from "./BasicFormFields";
import { ClaudeFormFields } from "./ClaudeFormFields";
import { CodexFormFields } from "./CodexFormFields";
import { GeminiFormFields } from "./GeminiFormFields";
import { OmoFormFields } from "./OmoFormFields";
import { type OmoGlobalConfigFieldsRef } from "./OmoGlobalConfigFields";
import { OmoCommonConfigEditor } from "./OmoCommonConfigEditor";
import * as configApi from "@/lib/api/config";
import type { OmoGlobalConfig } from "@/types/omo";
import { mergeOmoConfigPreview, parseOmoOtherFieldsObject } from "@/types/omo";
import {
  ProviderAdvancedConfig,
  type PricingModelSourceOption,
} from "./ProviderAdvancedConfig";
import {
  useProviderCategory,
  useApiKeyState,
  useBaseUrlState,
  useModelState,
  useCodexConfigState,
  useApiKeyLink,
  useTemplateValues,
  useCommonConfigSnippet,
  useCodexCommonConfig,
  useSpeedTestEndpoints,
  useCodexTomlValidation,
  useGeminiConfigState,
  useGeminiCommonConfig,
} from "./hooks";
import { useProvidersQuery } from "@/lib/query/queries";
import { useOmoGlobalConfig } from "@/lib/query/omo";

const CLAUDE_DEFAULT_CONFIG = JSON.stringify({ env: {} }, null, 2);
const CODEX_DEFAULT_CONFIG = JSON.stringify({ auth: {}, config: "" }, null, 2);
const GEMINI_DEFAULT_CONFIG = JSON.stringify(
  {
    env: {
      GOOGLE_GEMINI_BASE_URL: "",
      GEMINI_API_KEY: "",
      GEMINI_MODEL: "gemini-3-pro-preview",
    },
  },
  null,
  2,
);

const OPENCODE_DEFAULT_NPM = "@ai-sdk/openai-compatible";
const OPENCODE_DEFAULT_CONFIG = JSON.stringify(
  {
    npm: OPENCODE_DEFAULT_NPM,
    options: {
      baseURL: "",
      apiKey: "",
    },
    models: {},
  },
  null,
  2,
);
const OPENCODE_KNOWN_OPTION_KEYS = ["baseURL", "apiKey", "headers"] as const;
const isKnownOpencodeOptionKey = (key: string) =>
  OPENCODE_KNOWN_OPTION_KEYS.includes(
    key as (typeof OPENCODE_KNOWN_OPTION_KEYS)[number],
  );

function parseOpencodeConfig(
  settingsConfig?: Record<string, unknown>,
): OpenCodeProviderConfig {
  const normalize = (
    parsed: Partial<OpenCodeProviderConfig>,
  ): OpenCodeProviderConfig => ({
    npm: parsed.npm || OPENCODE_DEFAULT_NPM,
    options:
      parsed.options && typeof parsed.options === "object"
        ? (parsed.options as OpenCodeProviderConfig["options"])
        : {},
    models:
      parsed.models && typeof parsed.models === "object"
        ? (parsed.models as Record<string, OpenCodeModel>)
        : {},
  });

  try {
    const parsed = JSON.parse(
      settingsConfig ? JSON.stringify(settingsConfig) : OPENCODE_DEFAULT_CONFIG,
    ) as Partial<OpenCodeProviderConfig>;
    return normalize(parsed);
  } catch {
    return {
      npm: OPENCODE_DEFAULT_NPM,
      options: {},
      models: {},
    };
  }
}

function parseOpencodeConfigStrict(
  settingsConfig?: Record<string, unknown>,
): OpenCodeProviderConfig {
  const parsed = JSON.parse(
    settingsConfig ? JSON.stringify(settingsConfig) : OPENCODE_DEFAULT_CONFIG,
  ) as Partial<OpenCodeProviderConfig>;
  return {
    npm: parsed.npm || OPENCODE_DEFAULT_NPM,
    options:
      parsed.options && typeof parsed.options === "object"
        ? (parsed.options as OpenCodeProviderConfig["options"])
        : {},
    models:
      parsed.models && typeof parsed.models === "object"
        ? (parsed.models as Record<string, OpenCodeModel>)
        : {},
  };
}

function toOpencodeExtraOptions(
  options: OpenCodeProviderConfig["options"],
): Record<string, string> {
  const extra: Record<string, string> = {};
  for (const [k, v] of Object.entries(options || {})) {
    if (!isKnownOpencodeOptionKey(k)) {
      extra[k] = typeof v === "string" ? v : JSON.stringify(v);
    }
  }
  return extra;
}

const OPENCLAW_DEFAULT_CONFIG = JSON.stringify(
  {
    baseUrl: "",
    apiKey: "",
    api: "openai-completions",
    models: [],
  },
  null,
  2,
);

type PresetEntry = {
  id: string;
  preset:
    | ProviderPreset
    | CodexProviderPreset
    | GeminiProviderPreset
    | OpenCodeProviderPreset
    | OpenClawProviderPreset;
};

interface ProviderFormProps {
  appId: AppId;
  providerId?: string;
  submitLabel: string;
  onSubmit: (values: ProviderFormValues) => void;
  onCancel: () => void;
  onUniversalPresetSelect?: (preset: UniversalProviderPreset) => void;
  onManageUniversalProviders?: () => void;
  initialData?: {
    name?: string;
    websiteUrl?: string;
    notes?: string;
    settingsConfig?: Record<string, unknown>;
    category?: ProviderCategory;
    meta?: ProviderMeta;
    icon?: string;
    iconColor?: string;
  };
  showButtons?: boolean;
}

const normalizePricingSource = (value?: string): PricingModelSourceOption =>
  value === "request" || value === "response" ? value : "inherit";

function buildOmoProfilePreview(
  agents: Record<string, Record<string, unknown>>,
  categories: Record<string, Record<string, unknown>>,
  otherFieldsStr: string,
): Record<string, unknown> {
  const profileOnly: Record<string, unknown> = {};
  if (Object.keys(agents).length > 0) {
    profileOnly.agents = agents;
  }
  if (Object.keys(categories).length > 0) {
    profileOnly.categories = categories;
  }
  if (otherFieldsStr.trim()) {
    try {
      const other = parseOmoOtherFieldsObject(otherFieldsStr);
      if (other) {
        Object.assign(profileOnly, other);
      }
    } catch {}
  }
  return profileOnly;
}

const EMPTY_OMO_GLOBAL_CONFIG: OmoGlobalConfig = {
  id: "global",
  disabledAgents: [],
  disabledMcps: [],
  disabledHooks: [],
  disabledSkills: [],
  updatedAt: "",
};

export function ProviderForm({
  appId,
  providerId,
  submitLabel,
  onSubmit,
  onCancel,
  onUniversalPresetSelect,
  onManageUniversalProviders,
  initialData,
  showButtons = true,
}: ProviderFormProps) {
  const { t } = useTranslation();
  const isEditMode = Boolean(initialData);

  const [selectedPresetId, setSelectedPresetId] = useState<string | null>(
    initialData ? null : "custom",
  );
  const [activePreset, setActivePreset] = useState<{
    id: string;
    category?: ProviderCategory;
    isPartner?: boolean;
    partnerPromotionKey?: string;
    suggestedDefaults?: OpenClawSuggestedDefaults;
  } | null>(null);
  const [isEndpointModalOpen, setIsEndpointModalOpen] = useState(false);
  const [isCodexEndpointModalOpen, setIsCodexEndpointModalOpen] =
    useState(false);

  const [draftCustomEndpoints, setDraftCustomEndpoints] = useState<string[]>(
    () => {
      if (initialData) return [];
      return [];
    },
  );
  const [endpointAutoSelect, setEndpointAutoSelect] = useState<boolean>(
    () => initialData?.meta?.endpointAutoSelect ?? true,
  );

  const [testConfig, setTestConfig] = useState<ProviderTestConfig>(
    () => initialData?.meta?.testConfig ?? { enabled: false },
  );
  const [proxyConfig, setProxyConfig] = useState<ProviderProxyConfig>(
    () => initialData?.meta?.proxyConfig ?? { enabled: false },
  );
  const [pricingConfig, setPricingConfig] = useState<{
    enabled: boolean;
    costMultiplier?: string;
    pricingModelSource: PricingModelSourceOption;
  }>(() => ({
    enabled:
      initialData?.meta?.costMultiplier !== undefined ||
      initialData?.meta?.pricingModelSource !== undefined,
    costMultiplier: initialData?.meta?.costMultiplier,
    pricingModelSource: normalizePricingSource(
      initialData?.meta?.pricingModelSource,
    ),
  }));

  const { category } = useProviderCategory({
    appId,
    selectedPresetId,
    isEditMode,
    initialCategory: initialData?.category,
  });
  const isOmoCategory = appId === "opencode" && category === "omo";
  const { data: queriedOmoGlobalConfig } = useOmoGlobalConfig(isOmoCategory);

  useEffect(() => {
    setSelectedPresetId(initialData ? null : "custom");
    setActivePreset(null);

    if (!initialData) {
      setDraftCustomEndpoints([]);
    }
    setEndpointAutoSelect(initialData?.meta?.endpointAutoSelect ?? true);
    setTestConfig(initialData?.meta?.testConfig ?? { enabled: false });
    setProxyConfig(initialData?.meta?.proxyConfig ?? { enabled: false });
    setPricingConfig({
      enabled:
        initialData?.meta?.costMultiplier !== undefined ||
        initialData?.meta?.pricingModelSource !== undefined,
      costMultiplier: initialData?.meta?.costMultiplier,
      pricingModelSource: normalizePricingSource(
        initialData?.meta?.pricingModelSource,
      ),
    });
  }, [appId, initialData]);

  const defaultValues: ProviderFormData = useMemo(
    () => ({
      name: initialData?.name ?? "",
      websiteUrl: initialData?.websiteUrl ?? "",
      notes: initialData?.notes ?? "",
      settingsConfig: initialData?.settingsConfig
        ? JSON.stringify(initialData.settingsConfig, null, 2)
        : appId === "codex"
          ? CODEX_DEFAULT_CONFIG
          : appId === "gemini"
            ? GEMINI_DEFAULT_CONFIG
            : appId === "opencode"
              ? OPENCODE_DEFAULT_CONFIG
              : appId === "openclaw"
                ? OPENCLAW_DEFAULT_CONFIG
                : CLAUDE_DEFAULT_CONFIG,
      icon: initialData?.icon ?? "",
      iconColor: initialData?.iconColor ?? "",
    }),
    [initialData, appId],
  );

  const form = useForm<ProviderFormData>({
    resolver: zodResolver(providerSchema),
    defaultValues,
    mode: "onSubmit",
  });

  const {
    apiKey,
    handleApiKeyChange,
    showApiKey: shouldShowApiKey,
  } = useApiKeyState({
    initialConfig: form.getValues("settingsConfig"),
    onConfigChange: (config) => form.setValue("settingsConfig", config),
    selectedPresetId,
    category,
    appType: appId,
  });

  const { baseUrl, handleClaudeBaseUrlChange } = useBaseUrlState({
    appType: appId,
    category,
    settingsConfig: form.getValues("settingsConfig"),
    codexConfig: "",
    onSettingsConfigChange: (config) => form.setValue("settingsConfig", config),
    onCodexConfigChange: () => {},
  });

  const {
    claudeModel,
    reasoningModel,
    defaultHaikuModel,
    defaultSonnetModel,
    defaultOpusModel,
    handleModelChange,
  } = useModelState({
    settingsConfig: form.getValues("settingsConfig"),
    onConfigChange: (config) => form.setValue("settingsConfig", config),
  });

  const [localApiFormat, setLocalApiFormat] = useState<ClaudeApiFormat>(() => {
    if (appId !== "claude") return "anthropic";
    return initialData?.meta?.apiFormat ?? "anthropic";
  });

  const handleApiFormatChange = useCallback((format: ClaudeApiFormat) => {
    setLocalApiFormat(format);
  }, []);

  const {
    codexAuth,
    codexConfig,
    codexApiKey,
    codexBaseUrl,
    codexModelName,
    codexAuthError,
    setCodexAuth,
    handleCodexApiKeyChange,
    handleCodexBaseUrlChange,
    handleCodexModelNameChange,
    handleCodexConfigChange: originalHandleCodexConfigChange,
    resetCodexConfig,
  } = useCodexConfigState({ initialData });

  const { configError: codexConfigError, debouncedValidate } =
    useCodexTomlValidation();

  const handleCodexConfigChange = useCallback(
    (value: string) => {
      originalHandleCodexConfigChange(value);
      debouncedValidate(value);
    },
    [originalHandleCodexConfigChange, debouncedValidate],
  );

  useEffect(() => {
    if (appId === "codex" && !initialData && selectedPresetId === "custom") {
      const template = getCodexCustomTemplate();
      resetCodexConfig(template.auth, template.config);
    }
  }, [appId, initialData, selectedPresetId, resetCodexConfig]);

  useEffect(() => {
    form.reset(defaultValues);
  }, [defaultValues, form]);

  const presetCategoryLabels: Record<string, string> = useMemo(
    () => ({
      official: t("providerForm.categoryOfficial", {
        defaultValue: "官方",
      }),
      cn_official: t("providerForm.categoryCnOfficial", {
        defaultValue: "国内官方",
      }),
      aggregator: t("providerForm.categoryAggregation", {
        defaultValue: "聚合服务",
      }),
      third_party: t("providerForm.categoryThirdParty", {
        defaultValue: "第三方",
      }),
      omo: "OMO",
    }),
    [t],
  );

  const presetEntries = useMemo(() => {
    if (appId === "codex") {
      return codexProviderPresets.map<PresetEntry>((preset, index) => ({
        id: `codex-${index}`,
        preset,
      }));
    } else if (appId === "gemini") {
      return geminiProviderPresets.map<PresetEntry>((preset, index) => ({
        id: `gemini-${index}`,
        preset,
      }));
    } else if (appId === "opencode") {
      return opencodeProviderPresets.map<PresetEntry>((preset, index) => ({
        id: `opencode-${index}`,
        preset,
      }));
    } else if (appId === "openclaw") {
      return openclawProviderPresets.map<PresetEntry>((preset, index) => ({
        id: `openclaw-${index}`,
        preset,
      }));
    }
    return providerPresets.map<PresetEntry>((preset, index) => ({
      id: `claude-${index}`,
      preset,
    }));
  }, [appId]);

  const {
    templateValues,
    templateValueEntries,
    selectedPreset: templatePreset,
    handleTemplateValueChange,
    validateTemplateValues,
  } = useTemplateValues({
    selectedPresetId: appId === "claude" ? selectedPresetId : null,
    presetEntries: appId === "claude" ? presetEntries : [],
    settingsConfig: form.getValues("settingsConfig"),
    onConfigChange: (config) => form.setValue("settingsConfig", config),
  });

  const {
    useCommonConfig,
    commonConfigSnippet,
    commonConfigError,
    handleCommonConfigToggle,
    handleCommonConfigSnippetChange,
    isExtracting: isClaudeExtracting,
    handleExtract: handleClaudeExtract,
  } = useCommonConfigSnippet({
    settingsConfig: form.getValues("settingsConfig"),
    onConfigChange: (config) => form.setValue("settingsConfig", config),
    initialData: appId === "claude" ? initialData : undefined,
    selectedPresetId: selectedPresetId ?? undefined,
    enabled: appId === "claude",
  });

  const {
    useCommonConfig: useCodexCommonConfigFlag,
    commonConfigSnippet: codexCommonConfigSnippet,
    commonConfigError: codexCommonConfigError,
    handleCommonConfigToggle: handleCodexCommonConfigToggle,
    handleCommonConfigSnippetChange: handleCodexCommonConfigSnippetChange,
    isExtracting: isCodexExtracting,
    handleExtract: handleCodexExtract,
  } = useCodexCommonConfig({
    codexConfig,
    onConfigChange: handleCodexConfigChange,
    initialData: appId === "codex" ? initialData : undefined,
    selectedPresetId: selectedPresetId ?? undefined,
  });

  const {
    geminiEnv,
    geminiConfig,
    geminiApiKey,
    geminiBaseUrl,
    geminiModel,
    envError,
    configError: geminiConfigError,
    handleGeminiApiKeyChange: originalHandleGeminiApiKeyChange,
    handleGeminiBaseUrlChange: originalHandleGeminiBaseUrlChange,
    handleGeminiModelChange: originalHandleGeminiModelChange,
    handleGeminiEnvChange,
    handleGeminiConfigChange,
    resetGeminiConfig,
    envStringToObj,
    envObjToString,
  } = useGeminiConfigState({
    initialData: appId === "gemini" ? initialData : undefined,
  });

  const updateGeminiEnvField = useCallback(
    (
      key: "GEMINI_API_KEY" | "GOOGLE_GEMINI_BASE_URL" | "GEMINI_MODEL",
      value: string,
    ) => {
      try {
        const config = JSON.parse(form.getValues("settingsConfig") || "{}") as {
          env?: Record<string, unknown>;
        };
        if (!config.env || typeof config.env !== "object") {
          config.env = {};
        }
        config.env[key] = value;
        form.setValue("settingsConfig", JSON.stringify(config, null, 2));
      } catch {}
    },
    [form],
  );

  const handleGeminiApiKeyChange = useCallback(
    (key: string) => {
      originalHandleGeminiApiKeyChange(key);
      updateGeminiEnvField("GEMINI_API_KEY", key.trim());
    },
    [originalHandleGeminiApiKeyChange, updateGeminiEnvField],
  );

  const handleGeminiBaseUrlChange = useCallback(
    (url: string) => {
      originalHandleGeminiBaseUrlChange(url);
      updateGeminiEnvField(
        "GOOGLE_GEMINI_BASE_URL",
        url.trim().replace(/\/+$/, ""),
      );
    },
    [originalHandleGeminiBaseUrlChange, updateGeminiEnvField],
  );

  const handleGeminiModelChange = useCallback(
    (model: string) => {
      originalHandleGeminiModelChange(model);
      updateGeminiEnvField("GEMINI_MODEL", model.trim());
    },
    [originalHandleGeminiModelChange, updateGeminiEnvField],
  );

  const {
    useCommonConfig: useGeminiCommonConfigFlag,
    commonConfigSnippet: geminiCommonConfigSnippet,
    commonConfigError: geminiCommonConfigError,
    handleCommonConfigToggle: handleGeminiCommonConfigToggle,
    handleCommonConfigSnippetChange: handleGeminiCommonConfigSnippetChange,
    isExtracting: isGeminiExtracting,
    handleExtract: handleGeminiExtract,
  } = useGeminiCommonConfig({
    envValue: geminiEnv,
    onEnvChange: handleGeminiEnvChange,
    envStringToObj,
    envObjToString,
    initialData: appId === "gemini" ? initialData : undefined,
    selectedPresetId: selectedPresetId ?? undefined,
  });

  const { data: opencodeProvidersData } = useProvidersQuery("opencode");
  const existingOpencodeKeys = useMemo(() => {
    if (!opencodeProvidersData?.providers) return [];
    return Object.keys(opencodeProvidersData.providers).filter(
      (k) => k !== providerId,
    );
  }, [opencodeProvidersData?.providers, providerId]);
  const [enabledOpencodeProviderIds, setEnabledOpencodeProviderIds] = useState<
    string[] | null
  >(null);
  const [omoLiveIdsLoadFailed, setOmoLiveIdsLoadFailed] = useState(false);
  const lastOmoModelSourceWarningRef = useRef<string>("");

  useEffect(() => {
    let active = true;
    if (!isOmoCategory) {
      setEnabledOpencodeProviderIds(null);
      setOmoLiveIdsLoadFailed(false);
      return () => {
        active = false;
      };
    }

    setEnabledOpencodeProviderIds(null);
    setOmoLiveIdsLoadFailed(false);

    (async () => {
      try {
        const ids = await providersApi.getOpenCodeLiveProviderIds();
        if (active) {
          setEnabledOpencodeProviderIds(ids);
        }
      } catch (error) {
        console.warn(
          "[OMO_MODEL_SOURCE_LIVE_IDS_FAILED] failed to load live provider ids",
          error,
        );
        if (active) {
          setOmoLiveIdsLoadFailed(true);
          setEnabledOpencodeProviderIds(null);
        }
      }
    })();

    return () => {
      active = false;
    };
  }, [isOmoCategory]);

  const omoModelBuild = useMemo(() => {
    const empty = {
      options: [] as Array<{ value: string; label: string }>,
      variantsMap: {} as Record<string, string[]>,
      presetMetaMap: {} as Record<
        string,
        {
          options?: Record<string, unknown>;
          limit?: { context?: number; output?: number };
        }
      >,
      parseFailedProviders: [] as string[],
      usedFallbackSource: false,
    };
    if (!isOmoCategory) {
      return empty;
    }

    const allProviders = opencodeProvidersData?.providers;
    if (!allProviders) {
      return empty;
    }

    const shouldFilterByLive = !omoLiveIdsLoadFailed;
    if (shouldFilterByLive && enabledOpencodeProviderIds === null) {
      return empty;
    }
    const liveSet =
      shouldFilterByLive && enabledOpencodeProviderIds
        ? new Set(enabledOpencodeProviderIds)
        : null;

    const dedupedOptions = new Map<string, string>();
    const variantsMap: Record<string, string[]> = {};
    const presetMetaMap: Record<
      string,
      {
        options?: Record<string, unknown>;
        limit?: { context?: number; output?: number };
      }
    > = {};
    const parseFailedProviders: string[] = [];

    for (const [providerKey, provider] of Object.entries(allProviders)) {
      if (provider.category === "omo") {
        continue;
      }
      if (liveSet && !liveSet.has(providerKey)) {
        continue;
      }

      let parsedConfig: OpenCodeProviderConfig;
      try {
        parsedConfig = parseOpencodeConfigStrict(provider.settingsConfig);
      } catch (error) {
        parseFailedProviders.push(providerKey);
        console.warn(
          "[OMO_MODEL_SOURCE_PARSE_FAILED] failed to parse provider settings",
          {
            providerKey,
            error,
          },
        );
        continue;
      }
      for (const [modelId, model] of Object.entries(
        parsedConfig.models || {},
      )) {
        const modelName =
          typeof model.name === "string" && model.name.trim()
            ? model.name
            : modelId;
        const providerDisplayName =
          typeof provider.name === "string" && provider.name.trim()
            ? provider.name
            : providerKey;
        const value = `${providerKey}/${modelId}`;
        const label = `${providerDisplayName} / ${modelName} (${modelId})`;
        if (!dedupedOptions.has(value)) {
          dedupedOptions.set(value, label);
        }

        const rawVariants = model.variants;
        if (
          rawVariants &&
          typeof rawVariants === "object" &&
          !Array.isArray(rawVariants)
        ) {
          const variantKeys = Object.keys(rawVariants).filter(Boolean);
          if (variantKeys.length > 0) {
            variantsMap[value] = variantKeys;
          }
        }
      }

      // Preset fallback: for models without config-defined variants,
      // check if the npm package has preset variant definitions.
      // Also collect preset metadata (options, limit) for enrichment.
      const presetModels = OPENCODE_PRESET_MODEL_VARIANTS[parsedConfig.npm];
      if (presetModels) {
        for (const modelId of Object.keys(parsedConfig.models || {})) {
          const fullKey = `${providerKey}/${modelId}`;
          const preset = presetModels.find((p) => p.id === modelId);
          if (!preset) continue;

          // Variant fallback
          if (!variantsMap[fullKey] && preset.variants) {
            const presetKeys = Object.keys(preset.variants).filter(Boolean);
            if (presetKeys.length > 0) {
              variantsMap[fullKey] = presetKeys;
            }
          }

          // Collect preset metadata for model enrichment
          const meta: (typeof presetMetaMap)[string] = {};
          if (preset.options) meta.options = preset.options;
          if (preset.contextLimit || preset.outputLimit) {
            meta.limit = {};
            if (preset.contextLimit) meta.limit.context = preset.contextLimit;
            if (preset.outputLimit) meta.limit.output = preset.outputLimit;
          }
          if (Object.keys(meta).length > 0) {
            presetMetaMap[fullKey] = meta;
          }
        }
      }
    }

    return {
      options: Array.from(dedupedOptions.entries())
        .map(([value, label]) => ({ value, label }))
        .sort((a, b) => a.label.localeCompare(b.label, "zh-CN")),
      variantsMap,
      presetMetaMap,
      parseFailedProviders,
      usedFallbackSource: omoLiveIdsLoadFailed,
    };
  }, [
    isOmoCategory,
    opencodeProvidersData?.providers,
    enabledOpencodeProviderIds,
    omoLiveIdsLoadFailed,
  ]);
  const omoModelOptions = omoModelBuild.options;
  const omoModelVariantsMap = omoModelBuild.variantsMap;
  const omoPresetMetaMap = omoModelBuild.presetMetaMap;

  useEffect(() => {
    if (!isOmoCategory) return;
    const failed = omoModelBuild.parseFailedProviders;
    const fallback = omoModelBuild.usedFallbackSource;
    if (failed.length === 0 && !fallback) return;

    const signature = `${fallback ? "fallback:" : ""}${failed
      .slice()
      .sort()
      .join(",")}`;
    if (lastOmoModelSourceWarningRef.current === signature) return;
    lastOmoModelSourceWarningRef.current = signature;

    if (failed.length > 0) {
      toast.warning(
        t("omo.modelSourcePartialWarning", {
          count: failed.length,
          defaultValue:
            "Some provider model configs are invalid and were skipped.",
        }),
      );
    }
    if (fallback) {
      toast.warning(
        t("omo.modelSourceFallbackWarning", {
          defaultValue:
            "Failed to load live provider state. Falling back to configured providers.",
        }),
      );
    }
  }, [
    isOmoCategory,
    omoModelBuild.parseFailedProviders,
    omoModelBuild.usedFallbackSource,
    t,
  ]);

  const initialOmoSettings =
    appId === "opencode" && initialData?.category === "omo"
      ? (initialData.settingsConfig as Record<string, unknown> | undefined)
      : undefined;
  const initialOpencodeConfig =
    appId === "opencode"
      ? parseOpencodeConfig(initialData?.settingsConfig)
      : null;
  const initialOpencodeOptions = initialOpencodeConfig?.options || {};

  const [opencodeProviderKey, setOpencodeProviderKey] = useState<string>(() => {
    if (appId !== "opencode") return "";
    return providerId || "";
  });

  // OpenClaw: query existing providers for duplicate key checking
  const { data: openclawProvidersData } = useProvidersQuery("openclaw");
  const existingOpenclawKeys = useMemo(() => {
    if (!openclawProvidersData?.providers) return [];
    // Exclude current provider ID when in edit mode
    return Object.keys(openclawProvidersData.providers).filter(
      (k) => k !== providerId,
    );
  }, [openclawProvidersData?.providers, providerId]);

  // OpenClaw Provider Key state
  const [openclawProviderKey, setOpenclawProviderKey] = useState<string>(() => {
    if (appId !== "openclaw") return "";
    // In edit mode, use the existing provider ID as the key
    return providerId || "";
  });

  // OpenCode 配置状态
  const [opencodeNpm, setOpencodeNpm] = useState<string>(() => {
    if (appId !== "opencode") return OPENCODE_DEFAULT_NPM;
    return initialOpencodeConfig?.npm || OPENCODE_DEFAULT_NPM;
  });

  const [opencodeApiKey, setOpencodeApiKey] = useState<string>(() => {
    if (appId !== "opencode") return "";
    const value = initialOpencodeOptions.apiKey;
    return typeof value === "string" ? value : "";
  });

  const [opencodeBaseUrl, setOpencodeBaseUrl] = useState<string>(() => {
    if (appId !== "opencode") return "";
    const value = initialOpencodeOptions.baseURL;
    return typeof value === "string" ? value : "";
  });

  const [opencodeModels, setOpencodeModels] = useState<
    Record<string, OpenCodeModel>
  >(() => {
    if (appId !== "opencode") return {};
    return initialOpencodeConfig?.models || {};
  });

  const [opencodeExtraOptions, setOpencodeExtraOptions] = useState<
    Record<string, string>
  >(() => {
    if (appId !== "opencode") return {};
    return toOpencodeExtraOptions(initialOpencodeOptions);
  });

  const [omoAgents, setOmoAgents] = useState<
    Record<string, Record<string, unknown>>
  >(
    () =>
      (initialOmoSettings?.agents as Record<string, Record<string, unknown>>) ||
      {},
  );
  const [omoCategories, setOmoCategories] = useState<
    Record<string, Record<string, unknown>>
  >(
    () =>
      (initialOmoSettings?.categories as Record<
        string,
        Record<string, unknown>
      >) || {},
  );
  const [omoOtherFieldsStr, setOmoOtherFieldsStr] = useState(() => {
    const otherFields = initialOmoSettings?.otherFields;
    return otherFields ? JSON.stringify(otherFields, null, 2) : "";
  });

  const [omoGlobalState, setOmoGlobalState] = useState<OmoGlobalConfig | null>(
    null,
  );

  const [isOmoConfigModalOpen, setIsOmoConfigModalOpen] = useState(false);
  const [useOmoCommonConfig, setUseOmoCommonConfig] = useState(() => {
    const raw = initialOmoSettings?.useCommonConfig;
    return typeof raw === "boolean" ? raw : true;
  });
  const [isOmoSaving, setIsOmoSaving] = useState(false);
  const omoGlobalConfigRef = useRef<OmoGlobalConfigFieldsRef>(null);
  const [omoFieldsKey, setOmoFieldsKey] = useState(0);
  const effectiveOmoGlobalConfig =
    omoGlobalState ?? queriedOmoGlobalConfig ?? EMPTY_OMO_GLOBAL_CONFIG;

  const mergedOmoJsonPreview = useMemo(() => {
    if (useOmoCommonConfig) {
      const merged = mergeOmoConfigPreview(
        effectiveOmoGlobalConfig,
        omoAgents,
        omoCategories,
        omoOtherFieldsStr,
      );
      return JSON.stringify(merged, null, 2);
    } else {
      return JSON.stringify(
        buildOmoProfilePreview(omoAgents, omoCategories, omoOtherFieldsStr),
        null,
        2,
      );
    }
  }, [
    useOmoCommonConfig,
    effectiveOmoGlobalConfig,
    omoAgents,
    omoCategories,
    omoOtherFieldsStr,
  ]);

  useEffect(() => {
    if (appId !== "opencode" || category !== "omo" || isEditMode) return;
    let active = true;
    (async () => {
      let next = false;
      try {
        const raw = await configApi.getCommonConfigSnippet("omo");
        if (raw) {
          const parsed = JSON.parse(raw) as Record<string, unknown>;
          next = Object.keys(parsed).some(
            (k) => k !== "id" && k !== "updatedAt",
          );
        }
      } catch {}
      if (active) setUseOmoCommonConfig(next);
    })();
    return () => {
      active = false;
    };
  }, [appId, category, isEditMode]);

  const handleOmoGlobalConfigSave = useCallback(async () => {
    if (!omoGlobalConfigRef.current) return;
    setIsOmoSaving(true);
    try {
      const config = omoGlobalConfigRef.current.buildCurrentConfigStrict();
      await configApi.setCommonConfigSnippet("omo", JSON.stringify(config));
      setIsOmoConfigModalOpen(false);
      toast.success(
        t("omo.globalConfigSaved", { defaultValue: "Global config saved" }),
      );
    } catch (err) {
      toast.error(String(err));
    } finally {
      setIsOmoSaving(false);
    }
  }, [t]);

  const handleOmoEditClick = useCallback(() => {
    setOmoFieldsKey((k) => k + 1);
    setIsOmoConfigModalOpen(true);
  }, []);

  const resetOmoDraftState = useCallback((useCommonConfig = true) => {
    setOmoAgents({});
    setOmoCategories({});
    setOmoOtherFieldsStr("");
    setUseOmoCommonConfig(useCommonConfig);
  }, []);

  // OpenClaw 配置状态
  const [openclawBaseUrl, setOpenclawBaseUrl] = useState<string>(() => {
    if (appId !== "openclaw") return "";
    try {
      const config = JSON.parse(
        initialData?.settingsConfig
          ? JSON.stringify(initialData.settingsConfig)
          : OPENCLAW_DEFAULT_CONFIG,
      );
      return config.baseUrl || "";
    } catch {
      return "";
    }
  });

  const [openclawApiKey, setOpenclawApiKey] = useState<string>(() => {
    if (appId !== "openclaw") return "";
    try {
      const config = JSON.parse(
        initialData?.settingsConfig
          ? JSON.stringify(initialData.settingsConfig)
          : OPENCLAW_DEFAULT_CONFIG,
      );
      return config.apiKey || "";
    } catch {
      return "";
    }
  });

  const [openclawApi, setOpenclawApi] = useState<string>(() => {
    if (appId !== "openclaw") return "openai-completions";
    try {
      const config = JSON.parse(
        initialData?.settingsConfig
          ? JSON.stringify(initialData.settingsConfig)
          : OPENCLAW_DEFAULT_CONFIG,
      );
      return config.api || "openai-completions";
    } catch {
      return "openai-completions";
    }
  });

  const [openclawModels, setOpenclawModels] = useState<OpenClawModel[]>(() => {
    if (appId !== "openclaw") return [];
    try {
      const config = JSON.parse(
        initialData?.settingsConfig
          ? JSON.stringify(initialData.settingsConfig)
          : OPENCLAW_DEFAULT_CONFIG,
      );
      return config.models || [];
    } catch {
      return [];
    }
  });

  // OpenClaw handlers - sync state to form
  const handleOpenclawBaseUrlChange = useCallback(
    (baseUrl: string) => {
      setOpenclawBaseUrl(baseUrl);
      try {
        const config = JSON.parse(
          form.getValues("settingsConfig") || OPENCLAW_DEFAULT_CONFIG,
        );
        config.baseUrl = baseUrl.trim().replace(/\/+$/, "");
        form.setValue("settingsConfig", JSON.stringify(config, null, 2));
      } catch {
        // ignore
      }
    },
    [form],
  );

  const handleOpenclawApiKeyChange = useCallback(
    (apiKey: string) => {
      setOpenclawApiKey(apiKey);
      try {
        const config = JSON.parse(
          form.getValues("settingsConfig") || OPENCLAW_DEFAULT_CONFIG,
        );
        config.apiKey = apiKey;
        form.setValue("settingsConfig", JSON.stringify(config, null, 2));
      } catch {
        // ignore
      }
    },
    [form],
  );

  const handleOpenclawApiChange = useCallback(
    (api: string) => {
      setOpenclawApi(api);
      try {
        const config = JSON.parse(
          form.getValues("settingsConfig") || OPENCLAW_DEFAULT_CONFIG,
        );
        config.api = api;
        form.setValue("settingsConfig", JSON.stringify(config, null, 2));
      } catch {
        // ignore
      }
    },
    [form],
  );

  const handleOpenclawModelsChange = useCallback(
    (models: OpenClawModel[]) => {
      setOpenclawModels(models);
      try {
        const config = JSON.parse(
          form.getValues("settingsConfig") || OPENCLAW_DEFAULT_CONFIG,
        );
        config.models = models;
        form.setValue("settingsConfig", JSON.stringify(config, null, 2));
      } catch {
        // ignore
      }
    },
    [form],
  );

  const updateOpencodeSettings = useCallback(
    (updater: (config: Record<string, any>) => void) => {
      try {
        const config = JSON.parse(
          form.getValues("settingsConfig") || OPENCODE_DEFAULT_CONFIG,
        ) as Record<string, any>;
        updater(config);
        form.setValue("settingsConfig", JSON.stringify(config, null, 2));
      } catch {}
    },
    [form],
  );

  const handleOpencodeNpmChange = useCallback(
    (npm: string) => {
      setOpencodeNpm(npm);
      updateOpencodeSettings((config) => {
        config.npm = npm;
      });
    },
    [updateOpencodeSettings],
  );

  const handleOpencodeApiKeyChange = useCallback(
    (apiKey: string) => {
      setOpencodeApiKey(apiKey);
      updateOpencodeSettings((config) => {
        if (!config.options) config.options = {};
        config.options.apiKey = apiKey;
      });
    },
    [updateOpencodeSettings],
  );

  const handleOpencodeBaseUrlChange = useCallback(
    (baseUrl: string) => {
      setOpencodeBaseUrl(baseUrl);
      updateOpencodeSettings((config) => {
        if (!config.options) config.options = {};
        config.options.baseURL = baseUrl.trim().replace(/\/+$/, "");
      });
    },
    [updateOpencodeSettings],
  );

  const handleOpencodeModelsChange = useCallback(
    (models: Record<string, OpenCodeModel>) => {
      setOpencodeModels(models);
      updateOpencodeSettings((config) => {
        config.models = models;
      });
    },
    [updateOpencodeSettings],
  );

  const handleOpencodeExtraOptionsChange = useCallback(
    (options: Record<string, string>) => {
      setOpencodeExtraOptions(options);
      updateOpencodeSettings((config) => {
        if (!config.options) config.options = {};

        for (const k of Object.keys(config.options)) {
          if (!isKnownOpencodeOptionKey(k)) {
            delete config.options[k];
          }
        }

        for (const [k, v] of Object.entries(options)) {
          const trimmedKey = k.trim();
          if (trimmedKey && !trimmedKey.startsWith("option-")) {
            try {
              config.options[trimmedKey] = JSON.parse(v);
            } catch {
              config.options[trimmedKey] = v;
            }
          }
        }
      });
    },
    [updateOpencodeSettings],
  );

  const [isCommonConfigModalOpen, setIsCommonConfigModalOpen] = useState(false);

  const handleSubmit = (values: ProviderFormData) => {
    if (appId === "claude" && templateValueEntries.length > 0) {
      const validation = validateTemplateValues();
      if (!validation.isValid && validation.missingField) {
        toast.error(
          t("providerForm.fillParameter", {
            label: validation.missingField.label,
            defaultValue: `请填写 ${validation.missingField.label}`,
          }),
        );
        return;
      }
    }

    if (!values.name.trim()) {
      toast.error(
        t("providerForm.fillSupplierName", {
          defaultValue: "请填写供应商名称",
        }),
      );
      return;
    }

    if (appId === "opencode" && category !== "omo") {
      const keyPattern = /^[a-z0-9]+(-[a-z0-9]+)*$/;
      if (!opencodeProviderKey.trim()) {
        toast.error(t("opencode.providerKeyRequired"));
        return;
      }
      if (!keyPattern.test(opencodeProviderKey)) {
        toast.error(t("opencode.providerKeyInvalid"));
        return;
      }
      if (!isEditMode && existingOpencodeKeys.includes(opencodeProviderKey)) {
        toast.error(t("opencode.providerKeyDuplicate"));
        return;
      }
      if (Object.keys(opencodeModels).length === 0) {
        toast.error(t("opencode.modelsRequired"));
        return;
      }
    }

    // OpenClaw: validate provider key
    if (appId === "openclaw") {
      const keyPattern = /^[a-z0-9]+(-[a-z0-9]+)*$/;
      if (!openclawProviderKey.trim()) {
        toast.error(t("openclaw.providerKeyRequired"));
        return;
      }
      if (!keyPattern.test(openclawProviderKey)) {
        toast.error(t("openclaw.providerKeyInvalid"));
        return;
      }
      if (!isEditMode && existingOpenclawKeys.includes(openclawProviderKey)) {
        toast.error(t("openclaw.providerKeyDuplicate"));
        return;
      }
    }

    // 非官方供应商必填校验：端点和 API Key
    if (category !== "official") {
      if (appId === "claude") {
        if (!baseUrl.trim()) {
          toast.error(
            t("providerForm.endpointRequired", {
              defaultValue: "非官方供应商请填写 API 端点",
            }),
          );
          return;
        }
        if (!apiKey.trim()) {
          toast.error(
            t("providerForm.apiKeyRequired", {
              defaultValue: "非官方供应商请填写 API Key",
            }),
          );
          return;
        }
      } else if (appId === "codex") {
        if (!codexBaseUrl.trim()) {
          toast.error(
            t("providerForm.endpointRequired", {
              defaultValue: "非官方供应商请填写 API 端点",
            }),
          );
          return;
        }
        if (!codexApiKey.trim()) {
          toast.error(
            t("providerForm.apiKeyRequired", {
              defaultValue: "非官方供应商请填写 API Key",
            }),
          );
          return;
        }
      } else if (appId === "gemini") {
        if (!geminiBaseUrl.trim()) {
          toast.error(
            t("providerForm.endpointRequired", {
              defaultValue: "非官方供应商请填写 API 端点",
            }),
          );
          return;
        }
        if (!geminiApiKey.trim()) {
          toast.error(
            t("providerForm.apiKeyRequired", {
              defaultValue: "非官方供应商请填写 API Key",
            }),
          );
          return;
        }
      }
    }

    let settingsConfig: string;

    if (appId === "codex") {
      try {
        const authJson = JSON.parse(codexAuth);
        const configObj = {
          auth: authJson,
          config: codexConfig ?? "",
        };
        settingsConfig = JSON.stringify(configObj);
      } catch (err) {
        settingsConfig = values.settingsConfig.trim();
      }
    } else if (appId === "gemini") {
      try {
        const envObj = envStringToObj(geminiEnv);
        const configObj = geminiConfig.trim() ? JSON.parse(geminiConfig) : {};
        const combined = {
          env: envObj,
          config: configObj,
        };
        settingsConfig = JSON.stringify(combined);
      } catch (err) {
        settingsConfig = values.settingsConfig.trim();
      }
    } else if (appId === "opencode" && category === "omo") {
      const omoConfig: Record<string, unknown> = {};
      omoConfig.useCommonConfig = useOmoCommonConfig;
      if (Object.keys(omoAgents).length > 0) {
        omoConfig.agents = omoAgents;
      }
      if (Object.keys(omoCategories).length > 0) {
        omoConfig.categories = omoCategories;
      }
      if (omoOtherFieldsStr.trim()) {
        try {
          const otherFields = parseOmoOtherFieldsObject(omoOtherFieldsStr);
          if (!otherFields) {
            toast.error(
              t("omo.jsonMustBeObject", {
                field: t("omo.otherFields", {
                  defaultValue: "Other Config",
                }),
                defaultValue: "{{field}} must be a JSON object",
              }),
            );
            return;
          }
          omoConfig.otherFields = otherFields;
        } catch {
          toast.error(
            t("omo.invalidJson", {
              defaultValue: "Other Fields contains invalid JSON",
            }),
          );
          return;
        }
      }
      settingsConfig = JSON.stringify(omoConfig);
    } else {
      settingsConfig = values.settingsConfig.trim();
    }

    const payload: ProviderFormValues = {
      ...values,
      name: values.name.trim(),
      websiteUrl: values.websiteUrl?.trim() ?? "",
      settingsConfig,
    };

    if (appId === "opencode") {
      if (category === "omo") {
        if (!isEditMode) {
          payload.providerKey = `omo-${crypto.randomUUID().slice(0, 8)}`;
        }
      } else {
        payload.providerKey = opencodeProviderKey;
      }
    } else if (appId === "openclaw") {
      payload.providerKey = openclawProviderKey;
    }

    if (category === "omo" && !payload.presetCategory) {
      payload.presetCategory = "omo";
    }

    if (activePreset) {
      payload.presetId = activePreset.id;
      if (activePreset.category) {
        payload.presetCategory = activePreset.category;
      }
      if (activePreset.isPartner) {
        payload.isPartner = activePreset.isPartner;
      }
      // OpenClaw: 传递预设的 suggestedDefaults 到提交数据
      if (activePreset.suggestedDefaults) {
        payload.suggestedDefaults = activePreset.suggestedDefaults;
      }
    }

    if (!isEditMode && draftCustomEndpoints.length > 0) {
      const customEndpointsToSave: Record<
        string,
        import("@/types").CustomEndpoint
      > = draftCustomEndpoints.reduce(
        (acc, url) => {
          const now = Date.now();
          acc[url] = { url, addedAt: now, lastUsed: undefined };
          return acc;
        },
        {} as Record<string, import("@/types").CustomEndpoint>,
      );

      const hadEndpoints =
        initialData?.meta?.custom_endpoints &&
        Object.keys(initialData.meta.custom_endpoints).length > 0;
      const needsClearEndpoints =
        hadEndpoints && draftCustomEndpoints.length === 0;

      let mergedMeta = needsClearEndpoints
        ? mergeProviderMeta(initialData?.meta, {})
        : mergeProviderMeta(initialData?.meta, customEndpointsToSave);

      if (activePreset?.isPartner) {
        mergedMeta = {
          ...(mergedMeta ?? {}),
          isPartner: true,
        };
      }

      if (activePreset?.partnerPromotionKey) {
        mergedMeta = {
          ...(mergedMeta ?? {}),
          partnerPromotionKey: activePreset.partnerPromotionKey,
        };
      }

      if (mergedMeta !== undefined) {
        payload.meta = mergedMeta;
      }
    }

    const baseMeta: ProviderMeta | undefined =
      payload.meta ?? (initialData?.meta ? { ...initialData.meta } : undefined);
    payload.meta = {
      ...(baseMeta ?? {}),
      endpointAutoSelect,
      testConfig: testConfig.enabled ? testConfig : undefined,
      proxyConfig: proxyConfig.enabled ? proxyConfig : undefined,
      costMultiplier: pricingConfig.enabled
        ? pricingConfig.costMultiplier
        : undefined,
      pricingModelSource:
        pricingConfig.enabled && pricingConfig.pricingModelSource !== "inherit"
          ? pricingConfig.pricingModelSource
          : undefined,
      apiFormat:
        appId === "claude" && category !== "official"
          ? localApiFormat
          : undefined,
    };

    onSubmit(payload);
  };

  const groupedPresets = useMemo(() => {
    return presetEntries.reduce<Record<string, PresetEntry[]>>((acc, entry) => {
      const category = entry.preset.category ?? "others";
      if (!acc[category]) {
        acc[category] = [];
      }
      acc[category].push(entry);
      return acc;
    }, {});
  }, [presetEntries]);

  const categoryKeys = useMemo(() => {
    return Object.keys(groupedPresets).filter(
      (key) => key !== "custom" && groupedPresets[key]?.length,
    );
  }, [groupedPresets]);

  const shouldShowSpeedTest = category !== "official";

  const {
    shouldShowApiKeyLink: shouldShowClaudeApiKeyLink,
    websiteUrl: claudeWebsiteUrl,
    isPartner: isClaudePartner,
    partnerPromotionKey: claudePartnerPromotionKey,
  } = useApiKeyLink({
    appId: "claude",
    category,
    selectedPresetId,
    presetEntries,
    formWebsiteUrl: form.watch("websiteUrl") || "",
  });

  const {
    shouldShowApiKeyLink: shouldShowCodexApiKeyLink,
    websiteUrl: codexWebsiteUrl,
    isPartner: isCodexPartner,
    partnerPromotionKey: codexPartnerPromotionKey,
  } = useApiKeyLink({
    appId: "codex",
    category,
    selectedPresetId,
    presetEntries,
    formWebsiteUrl: form.watch("websiteUrl") || "",
  });

  const {
    shouldShowApiKeyLink: shouldShowGeminiApiKeyLink,
    websiteUrl: geminiWebsiteUrl,
    isPartner: isGeminiPartner,
    partnerPromotionKey: geminiPartnerPromotionKey,
  } = useApiKeyLink({
    appId: "gemini",
    category,
    selectedPresetId,
    presetEntries,
    formWebsiteUrl: form.watch("websiteUrl") || "",
  });

  const {
    shouldShowApiKeyLink: shouldShowOpencodeApiKeyLink,
    websiteUrl: opencodeWebsiteUrl,
    isPartner: isOpencodePartner,
    partnerPromotionKey: opencodePartnerPromotionKey,
  } = useApiKeyLink({
    appId: "opencode",
    category,
    selectedPresetId,
    presetEntries,
    formWebsiteUrl: form.watch("websiteUrl") || "",
  });

  // 使用 API Key 链接 hook (OpenClaw)
  const {
    shouldShowApiKeyLink: shouldShowOpenclawApiKeyLink,
    websiteUrl: openclawWebsiteUrl,
    isPartner: isOpenclawPartner,
    partnerPromotionKey: openclawPartnerPromotionKey,
  } = useApiKeyLink({
    appId: "openclaw",
    category,
    selectedPresetId,
    presetEntries,
    formWebsiteUrl: form.watch("websiteUrl") || "",
  });

  // 使用端点测速候选 hook
  const speedTestEndpoints = useSpeedTestEndpoints({
    appId,
    selectedPresetId,
    presetEntries,
    baseUrl,
    codexBaseUrl,
    initialData,
  });

  const handlePresetChange = (value: string) => {
    setSelectedPresetId(value);
    if (value === "custom") {
      setActivePreset(null);
      form.reset(defaultValues);

      if (appId === "codex") {
        const template = getCodexCustomTemplate();
        resetCodexConfig(template.auth, template.config);
      }
      if (appId === "gemini") {
        resetGeminiConfig({}, {});
      }
      if (appId === "opencode") {
        setOpencodeProviderKey("");
        setOpencodeNpm(OPENCODE_DEFAULT_NPM);
        setOpencodeBaseUrl("");
        setOpencodeApiKey("");
        setOpencodeModels({});
        setOpencodeExtraOptions({});
        resetOmoDraftState();
      }
      // OpenClaw 自定义模式：重置为空配置
      if (appId === "openclaw") {
        setOpenclawProviderKey("");
        setOpenclawBaseUrl("");
        setOpenclawApiKey("");
        setOpenclawApi("openai-completions");
        setOpenclawModels([]);
      }
      return;
    }

    const entry = presetEntries.find((item) => item.id === value);
    if (!entry) {
      return;
    }

    setActivePreset({
      id: value,
      category: entry.preset.category,
      isPartner: entry.preset.isPartner,
      partnerPromotionKey: entry.preset.partnerPromotionKey,
    });

    if (appId === "codex") {
      const preset = entry.preset as CodexProviderPreset;
      const auth = preset.auth ?? {};
      const config = preset.config ?? "";

      resetCodexConfig(auth, config);

      form.reset({
        name: preset.name,
        websiteUrl: preset.websiteUrl ?? "",
        settingsConfig: JSON.stringify({ auth, config }, null, 2),
        icon: preset.icon ?? "",
        iconColor: preset.iconColor ?? "",
      });
      return;
    }

    if (appId === "gemini") {
      const preset = entry.preset as GeminiProviderPreset;
      const env = (preset.settingsConfig as any)?.env ?? {};
      const config = (preset.settingsConfig as any)?.config ?? {};

      resetGeminiConfig(env, config);

      form.reset({
        name: preset.name,
        websiteUrl: preset.websiteUrl ?? "",
        settingsConfig: JSON.stringify(preset.settingsConfig, null, 2),
        icon: preset.icon ?? "",
        iconColor: preset.iconColor ?? "",
      });
      return;
    }

    if (appId === "opencode") {
      const preset = entry.preset as OpenCodeProviderPreset;
      const config = preset.settingsConfig;

      if (preset.category === "omo") {
        resetOmoDraftState();
        form.reset({
          name: "OMO",
          websiteUrl: preset.websiteUrl ?? "",
          settingsConfig: JSON.stringify({}, null, 2),
          icon: preset.icon ?? "",
          iconColor: preset.iconColor ?? "",
        });
        return;
      }

      setOpencodeProviderKey("");

      setOpencodeNpm(config.npm || OPENCODE_DEFAULT_NPM);
      setOpencodeBaseUrl(config.options?.baseURL || "");
      setOpencodeApiKey(config.options?.apiKey || "");
      setOpencodeModels(config.models || {});
      setOpencodeExtraOptions(toOpencodeExtraOptions(config.options || {}));

      form.reset({
        name: preset.name,
        websiteUrl: preset.websiteUrl ?? "",
        settingsConfig: JSON.stringify(config, null, 2),
        icon: preset.icon ?? "",
        iconColor: preset.iconColor ?? "",
      });
      return;
    }

    // OpenClaw preset handling
    if (appId === "openclaw") {
      const preset = entry.preset as OpenClawProviderPreset;
      const config = preset.settingsConfig;

      // Update activePreset with suggestedDefaults for OpenClaw
      setActivePreset({
        id: value,
        category: preset.category,
        isPartner: preset.isPartner,
        partnerPromotionKey: preset.partnerPromotionKey,
        suggestedDefaults: preset.suggestedDefaults,
      });

      // Clear provider key (user must enter their own unique key)
      setOpenclawProviderKey("");

      // Update OpenClaw-specific states
      setOpenclawBaseUrl(config.baseUrl || "");
      setOpenclawApiKey(config.apiKey || "");
      setOpenclawApi(config.api || "openai-completions");
      setOpenclawModels(config.models || []);

      // Update form fields
      form.reset({
        name: preset.name,
        websiteUrl: preset.websiteUrl ?? "",
        settingsConfig: JSON.stringify(config, null, 2),
        icon: preset.icon ?? "",
        iconColor: preset.iconColor ?? "",
      });
      return;
    }

    const preset = entry.preset as ProviderPreset;
    const config = applyTemplateValues(
      preset.settingsConfig,
      preset.templateValues,
    );

    if (preset.apiFormat) {
      setLocalApiFormat(preset.apiFormat);
    } else {
      setLocalApiFormat("anthropic");
    }

    form.reset({
      name: preset.name,
      websiteUrl: preset.websiteUrl ?? "",
      settingsConfig: JSON.stringify(config, null, 2),
      icon: preset.icon ?? "",
      iconColor: preset.iconColor ?? "",
    });
  };

  const settingsConfigErrorField = (
    <FormField
      control={form.control}
      name="settingsConfig"
      render={() => (
        <FormItem className="space-y-0">
          <FormMessage />
        </FormItem>
      )}
    />
  );

  return (
    <Form {...form}>
      <form
        id="provider-form"
        onSubmit={form.handleSubmit(handleSubmit)}
        className="space-y-6 glass rounded-xl p-6 border border-white/10"
      >
        {!initialData && (
          <ProviderPresetSelector
            selectedPresetId={selectedPresetId}
            groupedPresets={groupedPresets}
            categoryKeys={categoryKeys}
            presetCategoryLabels={presetCategoryLabels}
            onPresetChange={handlePresetChange}
            onUniversalPresetSelect={onUniversalPresetSelect}
            onManageUniversalProviders={onManageUniversalProviders}
            category={category}
          />
        )}

        <BasicFormFields
          form={form}
          beforeNameSlot={
            appId === "opencode" && category !== "omo" ? (
              <div className="space-y-2">
                <Label htmlFor="opencode-key">
                  {t("opencode.providerKey")}
                  <span className="text-destructive ml-1">*</span>
                </Label>
                <Input
                  id="opencode-key"
                  value={opencodeProviderKey}
                  onChange={(e) =>
                    setOpencodeProviderKey(
                      e.target.value.toLowerCase().replace(/[^a-z0-9-]/g, ""),
                    )
                  }
                  placeholder={t("opencode.providerKeyPlaceholder")}
                  disabled={isEditMode}
                  className={
                    (existingOpencodeKeys.includes(opencodeProviderKey) &&
                      !isEditMode) ||
                    (opencodeProviderKey.trim() !== "" &&
                      !/^[a-z0-9]+(-[a-z0-9]+)*$/.test(opencodeProviderKey))
                      ? "border-destructive"
                      : ""
                  }
                />
                {existingOpencodeKeys.includes(opencodeProviderKey) &&
                  !isEditMode && (
                    <p className="text-xs text-destructive">
                      {t("opencode.providerKeyDuplicate")}
                    </p>
                  )}
                {opencodeProviderKey.trim() !== "" &&
                  !/^[a-z0-9]+(-[a-z0-9]+)*$/.test(opencodeProviderKey) && (
                    <p className="text-xs text-destructive">
                      {t("opencode.providerKeyInvalid")}
                    </p>
                  )}
                {!(
                  existingOpencodeKeys.includes(opencodeProviderKey) &&
                  !isEditMode
                ) &&
                  (opencodeProviderKey.trim() === "" ||
                    /^[a-z0-9]+(-[a-z0-9]+)*$/.test(opencodeProviderKey)) && (
                    <p className="text-xs text-muted-foreground">
                      {t("opencode.providerKeyHint")}
                    </p>
                  )}
              </div>
            ) : appId === "openclaw" ? (
              <div className="space-y-2">
                <Label htmlFor="openclaw-key">
                  {t("openclaw.providerKey")}
                  <span className="text-destructive ml-1">*</span>
                </Label>
                <Input
                  id="openclaw-key"
                  value={openclawProviderKey}
                  onChange={(e) =>
                    setOpenclawProviderKey(
                      e.target.value.toLowerCase().replace(/[^a-z0-9-]/g, ""),
                    )
                  }
                  placeholder={t("openclaw.providerKeyPlaceholder")}
                  disabled={isEditMode}
                  className={
                    (existingOpenclawKeys.includes(openclawProviderKey) &&
                      !isEditMode) ||
                    (openclawProviderKey.trim() !== "" &&
                      !/^[a-z0-9]+(-[a-z0-9]+)*$/.test(openclawProviderKey))
                      ? "border-destructive"
                      : ""
                  }
                />
                {existingOpenclawKeys.includes(openclawProviderKey) &&
                  !isEditMode && (
                    <p className="text-xs text-destructive">
                      {t("openclaw.providerKeyDuplicate")}
                    </p>
                  )}
                {openclawProviderKey.trim() !== "" &&
                  !/^[a-z0-9]+(-[a-z0-9]+)*$/.test(openclawProviderKey) && (
                    <p className="text-xs text-destructive">
                      {t("openclaw.providerKeyInvalid")}
                    </p>
                  )}
                {!(
                  existingOpenclawKeys.includes(openclawProviderKey) &&
                  !isEditMode
                ) &&
                  (openclawProviderKey.trim() === "" ||
                    /^[a-z0-9]+(-[a-z0-9]+)*$/.test(openclawProviderKey)) && (
                    <p className="text-xs text-muted-foreground">
                      {t("openclaw.providerKeyHint")}
                    </p>
                  )}
              </div>
            ) : undefined
          }
        />

        {appId === "claude" && (
          <ClaudeFormFields
            providerId={providerId}
            shouldShowApiKey={shouldShowApiKey(
              form.getValues("settingsConfig"),
              isEditMode,
            )}
            apiKey={apiKey}
            onApiKeyChange={handleApiKeyChange}
            category={category}
            shouldShowApiKeyLink={shouldShowClaudeApiKeyLink}
            websiteUrl={claudeWebsiteUrl}
            isPartner={isClaudePartner}
            partnerPromotionKey={claudePartnerPromotionKey}
            templateValueEntries={templateValueEntries}
            templateValues={templateValues}
            templatePresetName={templatePreset?.name || ""}
            onTemplateValueChange={handleTemplateValueChange}
            shouldShowSpeedTest={shouldShowSpeedTest}
            baseUrl={baseUrl}
            onBaseUrlChange={handleClaudeBaseUrlChange}
            isEndpointModalOpen={isEndpointModalOpen}
            onEndpointModalToggle={setIsEndpointModalOpen}
            onCustomEndpointsChange={
              isEditMode ? undefined : setDraftCustomEndpoints
            }
            autoSelect={endpointAutoSelect}
            onAutoSelectChange={setEndpointAutoSelect}
            shouldShowModelSelector={category !== "official"}
            claudeModel={claudeModel}
            reasoningModel={reasoningModel}
            defaultHaikuModel={defaultHaikuModel}
            defaultSonnetModel={defaultSonnetModel}
            defaultOpusModel={defaultOpusModel}
            onModelChange={handleModelChange}
            speedTestEndpoints={speedTestEndpoints}
            apiFormat={localApiFormat}
            onApiFormatChange={handleApiFormatChange}
          />
        )}

        {appId === "codex" && (
          <CodexFormFields
            providerId={providerId}
            codexApiKey={codexApiKey}
            onApiKeyChange={handleCodexApiKeyChange}
            category={category}
            shouldShowApiKeyLink={shouldShowCodexApiKeyLink}
            websiteUrl={codexWebsiteUrl}
            isPartner={isCodexPartner}
            partnerPromotionKey={codexPartnerPromotionKey}
            shouldShowSpeedTest={shouldShowSpeedTest}
            codexBaseUrl={codexBaseUrl}
            onBaseUrlChange={handleCodexBaseUrlChange}
            isEndpointModalOpen={isCodexEndpointModalOpen}
            onEndpointModalToggle={setIsCodexEndpointModalOpen}
            onCustomEndpointsChange={
              isEditMode ? undefined : setDraftCustomEndpoints
            }
            autoSelect={endpointAutoSelect}
            onAutoSelectChange={setEndpointAutoSelect}
            shouldShowModelField={category !== "official"}
            modelName={codexModelName}
            onModelNameChange={handleCodexModelNameChange}
            speedTestEndpoints={speedTestEndpoints}
          />
        )}

        {appId === "gemini" && (
          <GeminiFormFields
            providerId={providerId}
            shouldShowApiKey={shouldShowApiKey(
              form.getValues("settingsConfig"),
              isEditMode,
            )}
            apiKey={geminiApiKey}
            onApiKeyChange={handleGeminiApiKeyChange}
            category={category}
            shouldShowApiKeyLink={shouldShowGeminiApiKeyLink}
            websiteUrl={geminiWebsiteUrl}
            isPartner={isGeminiPartner}
            partnerPromotionKey={geminiPartnerPromotionKey}
            shouldShowSpeedTest={shouldShowSpeedTest}
            baseUrl={geminiBaseUrl}
            onBaseUrlChange={handleGeminiBaseUrlChange}
            isEndpointModalOpen={isEndpointModalOpen}
            onEndpointModalToggle={setIsEndpointModalOpen}
            onCustomEndpointsChange={setDraftCustomEndpoints}
            autoSelect={endpointAutoSelect}
            onAutoSelectChange={setEndpointAutoSelect}
            shouldShowModelField={true}
            model={geminiModel}
            onModelChange={handleGeminiModelChange}
            speedTestEndpoints={speedTestEndpoints}
          />
        )}

        {appId === "opencode" && category !== "omo" && (
          <OpenCodeFormFields
            npm={opencodeNpm}
            onNpmChange={handleOpencodeNpmChange}
            apiKey={opencodeApiKey}
            onApiKeyChange={handleOpencodeApiKeyChange}
            category={category}
            shouldShowApiKeyLink={shouldShowOpencodeApiKeyLink}
            websiteUrl={opencodeWebsiteUrl}
            isPartner={isOpencodePartner}
            partnerPromotionKey={opencodePartnerPromotionKey}
            baseUrl={opencodeBaseUrl}
            onBaseUrlChange={handleOpencodeBaseUrlChange}
            models={opencodeModels}
            onModelsChange={handleOpencodeModelsChange}
            extraOptions={opencodeExtraOptions}
            onExtraOptionsChange={handleOpencodeExtraOptionsChange}
          />
        )}

        {appId === "opencode" && category === "omo" && (
          <OmoFormFields
            modelOptions={omoModelOptions}
            modelVariantsMap={omoModelVariantsMap}
            presetMetaMap={omoPresetMetaMap}
            agents={omoAgents}
            onAgentsChange={setOmoAgents}
            categories={omoCategories}
            onCategoriesChange={setOmoCategories}
            otherFieldsStr={omoOtherFieldsStr}
            onOtherFieldsStrChange={setOmoOtherFieldsStr}
          />
        )}

        {/* OpenClaw 专属字段 */}
        {appId === "openclaw" && (
          <OpenClawFormFields
            baseUrl={openclawBaseUrl}
            onBaseUrlChange={handleOpenclawBaseUrlChange}
            apiKey={openclawApiKey}
            onApiKeyChange={handleOpenclawApiKeyChange}
            category={category}
            shouldShowApiKeyLink={shouldShowOpenclawApiKeyLink}
            websiteUrl={openclawWebsiteUrl}
            isPartner={isOpenclawPartner}
            partnerPromotionKey={openclawPartnerPromotionKey}
            api={openclawApi}
            onApiChange={handleOpenclawApiChange}
            models={openclawModels}
            onModelsChange={handleOpenclawModelsChange}
          />
        )}

        {/* 配置编辑器：Codex、Claude、Gemini 分别使用不同的编辑器 */}
        {appId === "codex" ? (
          <>
            <CodexConfigEditor
              authValue={codexAuth}
              configValue={codexConfig}
              onAuthChange={setCodexAuth}
              onConfigChange={handleCodexConfigChange}
              useCommonConfig={useCodexCommonConfigFlag}
              onCommonConfigToggle={handleCodexCommonConfigToggle}
              commonConfigSnippet={codexCommonConfigSnippet}
              onCommonConfigSnippetChange={handleCodexCommonConfigSnippetChange}
              commonConfigError={codexCommonConfigError}
              authError={codexAuthError}
              configError={codexConfigError}
              onExtract={handleCodexExtract}
              isExtracting={isCodexExtracting}
            />
            {settingsConfigErrorField}
          </>
        ) : appId === "gemini" ? (
          <>
            <GeminiConfigEditor
              envValue={geminiEnv}
              configValue={geminiConfig}
              onEnvChange={handleGeminiEnvChange}
              onConfigChange={handleGeminiConfigChange}
              useCommonConfig={useGeminiCommonConfigFlag}
              onCommonConfigToggle={handleGeminiCommonConfigToggle}
              commonConfigSnippet={geminiCommonConfigSnippet}
              onCommonConfigSnippetChange={
                handleGeminiCommonConfigSnippetChange
              }
              commonConfigError={geminiCommonConfigError}
              envError={envError}
              configError={geminiConfigError}
              onExtract={handleGeminiExtract}
              isExtracting={isGeminiExtracting}
            />
            {settingsConfigErrorField}
          </>
        ) : appId === "opencode" && category === "omo" ? (
          <OmoCommonConfigEditor
            previewValue={mergedOmoJsonPreview}
            useCommonConfig={useOmoCommonConfig}
            onCommonConfigToggle={setUseOmoCommonConfig}
            isModalOpen={isOmoConfigModalOpen}
            onEditClick={handleOmoEditClick}
            onModalClose={() => setIsOmoConfigModalOpen(false)}
            onSave={handleOmoGlobalConfigSave}
            isSaving={isOmoSaving}
            onGlobalConfigStateChange={setOmoGlobalState}
            globalConfigRef={omoGlobalConfigRef}
            fieldsKey={omoFieldsKey}
          />
        ) : appId === "opencode" && category !== "omo" ? (
          <>
            <div className="space-y-2">
              <Label htmlFor="settingsConfig">{t("provider.configJson")}</Label>
              <JsonEditor
                value={form.getValues("settingsConfig")}
                onChange={(config) => form.setValue("settingsConfig", config)}
                placeholder={`{
  "npm": "@ai-sdk/openai-compatible",
  "options": {
    "baseURL": "https://your-api-endpoint.com",
    "apiKey": "your-api-key-here"
  },
  "models": {}
}`}
                rows={14}
                showValidation={true}
                language="json"
              />
            </div>
            {settingsConfigErrorField}
          </>
        ) : appId === "openclaw" ? (
          <>
            <div className="space-y-2">
              <Label htmlFor="settingsConfig">{t("provider.configJson")}</Label>
              <JsonEditor
                value={form.getValues("settingsConfig")}
                onChange={(config) => form.setValue("settingsConfig", config)}
                placeholder={`{
  "baseUrl": "https://api.example.com/v1",
  "apiKey": "your-api-key-here",
  "api": "openai-completions",
  "models": []
}`}
                rows={14}
                showValidation={true}
                language="json"
              />
            </div>
            <FormField
              control={form.control}
              name="settingsConfig"
              render={() => (
                <FormItem className="space-y-0">
                  <FormMessage />
                </FormItem>
              )}
            />
          </>
        ) : (
          <>
            <CommonConfigEditor
              value={form.getValues("settingsConfig")}
              onChange={(value) => form.setValue("settingsConfig", value)}
              useCommonConfig={useCommonConfig}
              onCommonConfigToggle={handleCommonConfigToggle}
              commonConfigSnippet={commonConfigSnippet}
              onCommonConfigSnippetChange={handleCommonConfigSnippetChange}
              commonConfigError={commonConfigError}
              onEditClick={() => setIsCommonConfigModalOpen(true)}
              isModalOpen={isCommonConfigModalOpen}
              onModalClose={() => setIsCommonConfigModalOpen(false)}
              onExtract={handleClaudeExtract}
              isExtracting={isClaudeExtracting}
            />
            {settingsConfigErrorField}
          </>
        )}

        {category !== "omo" && (
          <ProviderAdvancedConfig
            testConfig={testConfig}
            proxyConfig={proxyConfig}
            pricingConfig={pricingConfig}
            onTestConfigChange={setTestConfig}
            onProxyConfigChange={setProxyConfig}
            onPricingConfigChange={setPricingConfig}
          />
        )}

        {showButtons && (
          <div className="flex justify-end gap-2">
            <Button variant="outline" type="button" onClick={onCancel}>
              {t("common.cancel")}
            </Button>
            <Button type="submit">{submitLabel}</Button>
          </div>
        )}
      </form>
    </Form>
  );
}

export type ProviderFormValues = ProviderFormData & {
  presetId?: string;
  presetCategory?: ProviderCategory;
  isPartner?: boolean;
  meta?: ProviderMeta;
  providerKey?: string; // OpenCode/OpenClaw: user-defined provider key
  suggestedDefaults?: OpenClawSuggestedDefaults; // OpenClaw: suggested default model configuration
};
