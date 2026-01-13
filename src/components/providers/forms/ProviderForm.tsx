import { useEffect, useMemo, useState, useCallback } from "react";
import { useForm } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Form, FormField, FormItem, FormMessage } from "@/components/ui/form";
import { providerSchema, type ProviderFormData } from "@/lib/schemas/provider";
import type { AppId } from "@/lib/api";
import type { ProviderCategory, ProviderMeta } from "@/types";
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
import type { UniversalProviderPreset } from "@/config/universalProviderPresets";
import { applyTemplateValues } from "@/utils/providerConfigUtils";
import { mergeProviderMeta } from "@/utils/providerMetaUtils";
import { getCodexCustomTemplate } from "@/config/codexTemplates";
import CodexConfigEditor from "./CodexConfigEditor";
import { CommonConfigEditor } from "./CommonConfigEditor";
import GeminiConfigEditor from "./GeminiConfigEditor";
import { ProviderPresetSelector } from "./ProviderPresetSelector";
import { BasicFormFields } from "./BasicFormFields";
import { ClaudeFormFields } from "./ClaudeFormFields";
import { CodexFormFields } from "./CodexFormFields";
import { GeminiFormFields } from "./GeminiFormFields";
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

type PresetEntry = {
  id: string;
  preset: ProviderPreset | CodexProviderPreset | GeminiProviderPreset;
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
  } | null>(null);
  const [isEndpointModalOpen, setIsEndpointModalOpen] = useState(false);
  const [isCodexEndpointModalOpen, setIsCodexEndpointModalOpen] =
    useState(false);

  // 新建供应商：收集端点测速弹窗中的"自定义端点"，提交时一次性落盘到 meta.custom_endpoints
  // 编辑供应商：端点已通过 API 直接保存，不再需要此状态
  const [draftCustomEndpoints, setDraftCustomEndpoints] = useState<string[]>(
    () => {
      // 仅在新建模式下使用
      if (initialData) return [];
      return [];
    },
  );
  const [endpointAutoSelect, setEndpointAutoSelect] = useState<boolean>(
    () => initialData?.meta?.endpointAutoSelect ?? true,
  );

  // 使用 category hook
  const { category } = useProviderCategory({
    appId,
    selectedPresetId,
    isEditMode,
    initialCategory: initialData?.category,
  });

  useEffect(() => {
    setSelectedPresetId(initialData ? null : "custom");
    setActivePreset(null);

    // 编辑模式不需要恢复 draftCustomEndpoints，端点已通过 API 管理
    if (!initialData) {
      setDraftCustomEndpoints([]);
    }
    setEndpointAutoSelect(initialData?.meta?.endpointAutoSelect ?? true);
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

  const settingsConfigValue = form.watch("settingsConfig");

  // 使用 API Key hook
  const {
    apiKey,
    handleApiKeyChange,
    showApiKey: shouldShowApiKey,
  } = useApiKeyState({
    initialConfig: form.watch("settingsConfig"),
    onConfigChange: (config) => form.setValue("settingsConfig", config),
    selectedPresetId,
    category,
    appType: appId,
  });

  // 使用 Base URL hook (Claude, Codex, Gemini)
  const { baseUrl, handleClaudeBaseUrlChange } = useBaseUrlState({
    appType: appId,
    category,
    settingsConfig: form.watch("settingsConfig"),
    codexConfig: "",
    onSettingsConfigChange: (config) => form.setValue("settingsConfig", config),
    onCodexConfigChange: () => {
      /* noop */
    },
  });

  // 使用 Model hook（新：主模型 + 推理模型 + Haiku/Sonnet/Opus 默认模型）
  const {
    claudeModel,
    reasoningModel,
    defaultHaikuModel,
    defaultSonnetModel,
    defaultOpusModel,
    handleModelChange,
  } = useModelState({
    settingsConfig: form.watch("settingsConfig"),
    onConfigChange: (config) => form.setValue("settingsConfig", config),
  });

  const isOpenRouterProvider = useMemo(() => {
    if (appId !== "claude") return false;
    const normalized = baseUrl.trim().toLowerCase();
    if (normalized.includes("openrouter.ai")) {
      return true;
    }
    try {
      const config = JSON.parse(settingsConfigValue || "{}");
      const envUrl = config?.env?.ANTHROPIC_BASE_URL;
      return typeof envUrl === "string" && envUrl.includes("openrouter.ai");
    } catch {
      return false;
    }
  }, [appId, baseUrl, settingsConfigValue]);

  const openRouterCompatEnabled = useMemo(() => {
    if (!isOpenRouterProvider) return false;
    try {
      const config = JSON.parse(settingsConfigValue || "{}");
      const raw = config?.openrouter_compat_mode;
      if (typeof raw === "boolean") return raw;
      if (typeof raw === "number") return raw !== 0;
      if (typeof raw === "string") {
        const normalized = raw.trim().toLowerCase();
        return normalized === "true" || normalized === "1";
      }
    } catch {
      // ignore
    }
    return false; // OpenRouter now supports Claude Code compatible API, no need for transform
  }, [isOpenRouterProvider, settingsConfigValue]);

  const handleOpenRouterCompatChange = useCallback(
    (enabled: boolean) => {
      try {
        const currentConfig = JSON.parse(
          form.getValues("settingsConfig") || "{}",
        );
        currentConfig.openrouter_compat_mode = enabled;
        form.setValue("settingsConfig", JSON.stringify(currentConfig, null, 2));
      } catch {
        // ignore
      }
    },
    [form],
  );

  // 使用 Codex 配置 hook (仅 Codex 模式)
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

  // 使用 Codex TOML 校验 hook (仅 Codex 模式)
  const { configError: codexConfigError, debouncedValidate } =
    useCodexTomlValidation();

  // 包装 handleCodexConfigChange，添加实时校验
  const handleCodexConfigChange = useCallback(
    (value: string) => {
      originalHandleCodexConfigChange(value);
      debouncedValidate(value);
    },
    [originalHandleCodexConfigChange, debouncedValidate],
  );

  // Codex 新建模式：初始化时自动填充模板
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
    }
    return providerPresets.map<PresetEntry>((preset, index) => ({
      id: `claude-${index}`,
      preset,
    }));
  }, [appId]);

  // 使用模板变量 hook (仅 Claude 模式)
  const {
    templateValues,
    templateValueEntries,
    selectedPreset: templatePreset,
    handleTemplateValueChange,
    validateTemplateValues,
  } = useTemplateValues({
    selectedPresetId: appId === "claude" ? selectedPresetId : null,
    presetEntries: appId === "claude" ? presetEntries : [],
    settingsConfig: form.watch("settingsConfig"),
    onConfigChange: (config) => form.setValue("settingsConfig", config),
  });

  // 使用通用配置片段 hook (仅 Claude 模式)
  const {
    useCommonConfig,
    commonConfigSnippet,
    commonConfigError,
    handleCommonConfigToggle,
    handleCommonConfigSnippetChange,
    isExtracting: isClaudeExtracting,
    handleExtract: handleClaudeExtract,
  } = useCommonConfigSnippet({
    settingsConfig: form.watch("settingsConfig"),
    onConfigChange: (config) => form.setValue("settingsConfig", config),
    initialData: appId === "claude" ? initialData : undefined,
    selectedPresetId: selectedPresetId ?? undefined,
  });

  // 使用 Codex 通用配置片段 hook (仅 Codex 模式)
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

  // 使用 Gemini 配置 hook (仅 Gemini 模式)
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

  // 包装 Gemini handlers 以同步 settingsConfig
  const handleGeminiApiKeyChange = useCallback(
    (key: string) => {
      originalHandleGeminiApiKeyChange(key);
      // 同步更新 settingsConfig
      try {
        const config = JSON.parse(form.watch("settingsConfig") || "{}");
        if (!config.env) config.env = {};
        config.env.GEMINI_API_KEY = key.trim();
        form.setValue("settingsConfig", JSON.stringify(config, null, 2));
      } catch {
        // ignore
      }
    },
    [originalHandleGeminiApiKeyChange, form],
  );

  const handleGeminiBaseUrlChange = useCallback(
    (url: string) => {
      originalHandleGeminiBaseUrlChange(url);
      // 同步更新 settingsConfig
      try {
        const config = JSON.parse(form.watch("settingsConfig") || "{}");
        if (!config.env) config.env = {};
        config.env.GOOGLE_GEMINI_BASE_URL = url.trim().replace(/\/+$/, "");
        form.setValue("settingsConfig", JSON.stringify(config, null, 2));
      } catch {
        // ignore
      }
    },
    [originalHandleGeminiBaseUrlChange, form],
  );

  const handleGeminiModelChange = useCallback(
    (model: string) => {
      originalHandleGeminiModelChange(model);
      // 同步更新 settingsConfig
      try {
        const config = JSON.parse(form.watch("settingsConfig") || "{}");
        if (!config.env) config.env = {};
        config.env.GEMINI_MODEL = model.trim();
        form.setValue("settingsConfig", JSON.stringify(config, null, 2));
      } catch {
        // ignore
      }
    },
    [originalHandleGeminiModelChange, form],
  );

  // 使用 Gemini 通用配置 hook (仅 Gemini 模式)
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

  const [isCommonConfigModalOpen, setIsCommonConfigModalOpen] = useState(false);

  const handleSubmit = (values: ProviderFormData) => {
    // 验证模板变量（仅 Claude 模式）
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

    // 供应商名称必填校验
    if (!values.name.trim()) {
      toast.error(
        t("providerForm.fillSupplierName", {
          defaultValue: "请填写供应商名称",
        }),
      );
      return;
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

    // Codex: 组合 auth 和 config
    if (appId === "codex") {
      try {
        const authJson = JSON.parse(codexAuth);
        const configObj = {
          auth: authJson,
          config: codexConfig ?? "",
        };
        settingsConfig = JSON.stringify(configObj);
      } catch (err) {
        // 如果解析失败，使用表单中的配置
        settingsConfig = values.settingsConfig.trim();
      }
    } else if (appId === "gemini") {
      // Gemini: 组合 env 和 config
      try {
        const envObj = envStringToObj(geminiEnv);
        const configObj = geminiConfig.trim() ? JSON.parse(geminiConfig) : {};
        const combined = {
          env: envObj,
          config: configObj,
        };
        settingsConfig = JSON.stringify(combined);
      } catch (err) {
        // 如果解析失败，使用表单中的配置
        settingsConfig = values.settingsConfig.trim();
      }
    } else {
      // Claude: 使用表单配置
      settingsConfig = values.settingsConfig.trim();
    }

    const payload: ProviderFormValues = {
      ...values,
      name: values.name.trim(),
      websiteUrl: values.websiteUrl?.trim() ?? "",
      settingsConfig,
    };

    if (activePreset) {
      payload.presetId = activePreset.id;
      if (activePreset.category) {
        payload.presetCategory = activePreset.category;
      }
      // 继承合作伙伴标识
      if (activePreset.isPartner) {
        payload.isPartner = activePreset.isPartner;
      }
    }

    // 处理 meta 字段：仅在新建模式下从 draftCustomEndpoints 生成 custom_endpoints
    // 编辑模式：端点已通过 API 直接保存，不在此处理
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

      // 检测是否需要清空端点（重要：区分"用户清空端点"和"用户没有修改端点"）
      const hadEndpoints =
        initialData?.meta?.custom_endpoints &&
        Object.keys(initialData.meta.custom_endpoints).length > 0;
      const needsClearEndpoints =
        hadEndpoints && draftCustomEndpoints.length === 0;

      // 如果用户明确清空了端点，传递空对象（而不是 null）让后端知道要删除
      let mergedMeta = needsClearEndpoints
        ? mergeProviderMeta(initialData?.meta, {})
        : mergeProviderMeta(initialData?.meta, customEndpointsToSave);

      // 添加合作伙伴标识与促销 key
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

  // 判断是否显示端点测速（仅官方类别不显示）
  const shouldShowSpeedTest = category !== "official";

  // 使用 API Key 链接 hook (Claude)
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

  // 使用 API Key 链接 hook (Codex)
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

  // 使用 API Key 链接 hook (Gemini)
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

      // Codex 自定义模式：加载模板
      if (appId === "codex") {
        const template = getCodexCustomTemplate();
        resetCodexConfig(template.auth, template.config);
      }
      // Gemini 自定义模式：重置为空配置
      if (appId === "gemini") {
        resetGeminiConfig({}, {});
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

      // 重置 Codex 配置
      resetCodexConfig(auth, config);

      // 更新表单其他字段
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

      // 重置 Gemini 配置
      resetGeminiConfig(env, config);

      // 更新表单其他字段
      form.reset({
        name: preset.name,
        websiteUrl: preset.websiteUrl ?? "",
        settingsConfig: JSON.stringify(preset.settingsConfig, null, 2),
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

    form.reset({
      name: preset.name,
      websiteUrl: preset.websiteUrl ?? "",
      settingsConfig: JSON.stringify(config, null, 2),
      icon: preset.icon ?? "",
      iconColor: preset.iconColor ?? "",
    });
  };

  return (
    <Form {...form}>
      <form
        id="provider-form"
        onSubmit={form.handleSubmit(handleSubmit)}
        className="space-y-6 glass rounded-xl p-6 border border-white/10"
      >
        {/* 预设供应商选择（仅新增模式显示） */}
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

        {/* 基础字段 */}
        <BasicFormFields form={form} />

        {/* Claude 专属字段 */}
        {appId === "claude" && (
          <ClaudeFormFields
            providerId={providerId}
            shouldShowApiKey={shouldShowApiKey(
              form.watch("settingsConfig"),
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
            showOpenRouterCompatToggle={false}
            openRouterCompatEnabled={openRouterCompatEnabled}
            onOpenRouterCompatChange={handleOpenRouterCompatChange}
          />
        )}

        {/* Codex 专属字段 */}
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

        {/* Gemini 专属字段 */}
        {appId === "gemini" && (
          <GeminiFormFields
            providerId={providerId}
            shouldShowApiKey={shouldShowApiKey(
              form.watch("settingsConfig"),
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
            {/* 配置验证错误显示 */}
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
            {/* 配置验证错误显示 */}
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
              value={form.watch("settingsConfig")}
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
            {/* 配置验证错误显示 */}
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
};
