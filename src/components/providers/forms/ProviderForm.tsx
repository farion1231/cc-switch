import { useEffect, useMemo, useState, useCallback } from "react";
import { useForm } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Form } from "@/components/ui/form";
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
import { applyTemplateValues } from "@/utils/providerConfigUtils";
import { mergeProviderMeta } from "@/utils/providerMetaUtils";
import CodexConfigEditor from "./CodexConfigEditor";
import { CommonConfigEditor } from "./CommonConfigEditor";
import { ProviderPresetSelector } from "./ProviderPresetSelector";
import { BasicFormFields } from "./BasicFormFields";
import { ClaudeFormFields } from "./ClaudeFormFields";
import { CodexFormFields } from "./CodexFormFields";
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
} from "./hooks";

const CLAUDE_DEFAULT_CONFIG = JSON.stringify({ env: {} }, null, 2);
const CODEX_DEFAULT_CONFIG = JSON.stringify({ auth: {}, config: "" }, null, 2);

type PresetEntry = {
  id: string;
  preset: ProviderPreset | CodexProviderPreset;
};

interface ProviderFormProps {
  appId: AppId;
  providerId?: string;
  submitLabel: string;
  onSubmit: (values: ProviderFormValues) => void;
  onCancel: () => void;
  initialData?: {
    name?: string;
    websiteUrl?: string;
    settingsConfig?: Record<string, unknown>;
    category?: ProviderCategory;
    meta?: ProviderMeta;
  };
  showButtons?: boolean;
}

export function ProviderForm({
  appId,
  providerId,
  submitLabel,
  onSubmit,
  onCancel,
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
  } | null>(null);
  const [isEndpointModalOpen, setIsEndpointModalOpen] = useState(false);

  // 新建供应商：收集端点测速弹窗中的"自定义端点"，提交时一次性落盘到 meta.custom_endpoints
  // 编辑供应商：从 initialData.meta.custom_endpoints 恢复端点列表
  const [draftCustomEndpoints, setDraftCustomEndpoints] = useState<string[]>(
    () => {
      if (!initialData?.meta?.custom_endpoints) {
        return [];
      }
      // 从 Record<string, CustomEndpoint> 中提取 URL 列表
      return Object.keys(initialData.meta.custom_endpoints);
    },
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

    // 重新初始化 draftCustomEndpoints（编辑模式时从 meta 恢复）
    if (initialData?.meta?.custom_endpoints) {
      setDraftCustomEndpoints(Object.keys(initialData.meta.custom_endpoints));
    } else {
      setDraftCustomEndpoints([]);
    }
  }, [appId, initialData]);

  const defaultValues: ProviderFormData = useMemo(
    () => ({
      name: initialData?.name ?? "",
      websiteUrl: initialData?.websiteUrl ?? "",
      settingsConfig: initialData?.settingsConfig
        ? JSON.stringify(initialData.settingsConfig, null, 2)
        : appId === "codex"
          ? CODEX_DEFAULT_CONFIG
          : CLAUDE_DEFAULT_CONFIG,
    }),
    [initialData, appId],
  );

  const form = useForm<ProviderFormData>({
    resolver: zodResolver(providerSchema),
    defaultValues,
    mode: "onSubmit",
  });

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
  });

  // 使用 Base URL hook (仅 Claude 模式)
  const { baseUrl, handleClaudeBaseUrlChange } = useBaseUrlState({
    appType: appId,
    category,
    settingsConfig: form.watch("settingsConfig"),
    codexConfig: "",
    onSettingsConfigChange: (config) => form.setValue("settingsConfig", config),
    onCodexConfigChange: () => {
      // Codex 使用 useCodexConfigState 管理 Base URL
    },
  });

  // 使用 Model hook（新：主模型 + Haiku/Sonnet/Opus 默认模型）
  const {
    claudeModel,
    defaultHaikuModel,
    defaultSonnetModel,
    defaultOpusModel,
    handleModelChange,
  } = useModelState({
    settingsConfig: form.watch("settingsConfig"),
    onConfigChange: (config) => form.setValue("settingsConfig", config),
  });

  // 使用 Codex 配置 hook (仅 Codex 模式)
  const {
    codexAuth,
    codexConfig,
    codexApiKey,
    codexBaseUrl,
    codexAuthError,
    setCodexAuth,
    handleCodexApiKeyChange,
    handleCodexBaseUrlChange,
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

  const [isCodexEndpointModalOpen, setIsCodexEndpointModalOpen] =
    useState(false);
  const [isCodexTemplateModalOpen, setIsCodexTemplateModalOpen] =
    useState(false);

  useEffect(() => {
    form.reset(defaultValues);
  }, [defaultValues, form]);

  const presetCategoryLabels: Record<string, string> = useMemo(
    () => ({
      official: t("providerPreset.categoryOfficial", {
        defaultValue: "官方",
      }),
      cn_official: t("providerPreset.categoryCnOfficial", {
        defaultValue: "国内官方",
      }),
      aggregator: t("providerPreset.categoryAggregator", {
        defaultValue: "聚合服务",
      }),
      third_party: t("providerPreset.categoryThirdParty", {
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
  } = useCommonConfigSnippet({
    settingsConfig: form.watch("settingsConfig"),
    onConfigChange: (config) => form.setValue("settingsConfig", config),
    initialData: appId === "claude" ? initialData : undefined,
  });

  // 使用 Codex 通用配置片段 hook (仅 Codex 模式)
  const {
    useCommonConfig: useCodexCommonConfigFlag,
    commonConfigSnippet: codexCommonConfigSnippet,
    commonConfigError: codexCommonConfigError,
    handleCommonConfigToggle: handleCodexCommonConfigToggle,
    handleCommonConfigSnippetChange: handleCodexCommonConfigSnippetChange,
  } = useCodexCommonConfig({
    codexConfig,
    onConfigChange: handleCodexConfigChange,
    initialData: appId === "codex" ? initialData : undefined,
  });

  const [isCommonConfigModalOpen, setIsCommonConfigModalOpen] = useState(false);

  const handleSubmit = (values: ProviderFormData) => {
    // 验证模板变量（仅 Claude 模式）
    if (appId === "claude" && templateValueEntries.length > 0) {
      const validation = validateTemplateValues();
      if (!validation.isValid && validation.missingField) {
        form.setError("settingsConfig", {
          type: "manual",
          message: t("providerForm.fillParameter", {
            label: validation.missingField.label,
            defaultValue: `���填写 ${validation.missingField.label}`,
          }),
        });
        return;
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

    // 处理 meta 字段：基于 draftCustomEndpoints 生成 custom_endpoints
    // 注意：不使用 customEndpointsMap，因为它包含了候选端点（预设、Base URL 等）
    // 而我们只需要保存用户真正添加的自定义端点
    const customEndpointsToSave: Record<
      string,
      import("@/types").CustomEndpoint
    > | null =
      draftCustomEndpoints.length > 0
        ? draftCustomEndpoints.reduce(
            (acc, url) => {
              // 尝试从 initialData.meta 中获取原有的端点元数据（保留 addedAt 和 lastUsed）
              const existing = initialData?.meta?.custom_endpoints?.[url];
              if (existing) {
                acc[url] = existing;
              } else {
                // 新端点：使用当前时间戳
                const now = Date.now();
                acc[url] = { url, addedAt: now, lastUsed: undefined };
              }
              return acc;
            },
            {} as Record<string, import("@/types").CustomEndpoint>,
          )
        : null;

    // 检测是否需要清空端点（重要：区分"用户清空端点"和"用户没有修改端点"）
    const hadEndpoints =
      initialData?.meta?.custom_endpoints &&
      Object.keys(initialData.meta.custom_endpoints).length > 0;
    const needsClearEndpoints =
      hadEndpoints && draftCustomEndpoints.length === 0;

    // 如果用户明确清空了端点，传递空对象（而不是 null）让后端知道要删除
    const mergedMeta = needsClearEndpoints
      ? mergeProviderMeta(initialData?.meta, {})
      : mergeProviderMeta(initialData?.meta, customEndpointsToSave);

    if (mergedMeta) {
      payload.meta = mergedMeta;
    }

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

      // Codex 自定义模式：重置为空配置
      if (appId === "codex") {
        resetCodexConfig({}, "");
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
    });
  };

  return (
    <Form {...form}>
      <form
        id="provider-form"
        onSubmit={form.handleSubmit(handleSubmit)}
        className="space-y-6"
      >
        {/* 预设供应商选择（仅新增模式显示） */}
        {!initialData && (
          <ProviderPresetSelector
            selectedPresetId={selectedPresetId}
            groupedPresets={groupedPresets}
            categoryKeys={categoryKeys}
            presetCategoryLabels={presetCategoryLabels}
            onPresetChange={handlePresetChange}
            category={category}
            appId={appId}
            onOpenWizard={
              appId === "codex"
                ? () => setIsCodexTemplateModalOpen(true)
                : undefined
            }
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
            onCustomEndpointsChange={setDraftCustomEndpoints}
            shouldShowModelSelector={category !== "official"}
            claudeModel={claudeModel}
            defaultHaikuModel={defaultHaikuModel}
            defaultSonnetModel={defaultSonnetModel}
            defaultOpusModel={defaultOpusModel}
            onModelChange={handleModelChange}
            speedTestEndpoints={speedTestEndpoints}
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
            onCustomEndpointsChange={setDraftCustomEndpoints}
            speedTestEndpoints={speedTestEndpoints}
          />
        )}

        {/* 配置编辑器：Claude 使用通用配置编辑器，Codex 使用专用编辑器 */}
        {appId === "codex" ? (
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
            onWebsiteUrlChange={(url) => form.setValue("websiteUrl", url)}
            onNameChange={(name) => form.setValue("name", name)}
            isTemplateModalOpen={isCodexTemplateModalOpen}
            setIsTemplateModalOpen={setIsCodexTemplateModalOpen}
          />
        ) : (
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
};
