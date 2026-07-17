import { useEffect, useMemo, useState } from "react";
import { useForm } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import {
  Form,
  FormControl,
  FormField,
  FormItem,
  FormLabel,
  FormMessage,
} from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import JsonEditor from "@/components/JsonEditor";
import { useDarkMode } from "@/hooks/useDarkMode";
import { providerSchema, type ProviderFormData } from "@/lib/schemas/provider";
import {
  buildLocalProxyRequestOverrides,
  formatRequestOverrideObject,
} from "@/lib/requestOverrides";
import type {
  ClaudeApiKeyField,
  CodexApiFormat,
  CodexChatReasoning,
  PromptCacheRoutingMode,
  ProviderCategory,
  ProviderMeta,
} from "@/types";
import type { ProviderFormProps, ProviderFormValues } from "./ProviderForm";
import { BasicFormFields } from "./BasicFormFields";
import { CodexFormFields } from "./CodexFormFields";
import { ProviderPresetSelector } from "./ProviderPresetSelector";
import {
  codexProviderPresets,
  type CodexProviderPreset,
} from "@/config/codexProviderPresets";
import {
  codexApiFormatFromWireApi,
  extractCodexBaseUrl,
  extractCodexModelName,
  extractCodexWireApi,
} from "@/utils/providerConfigUtils";
import {
  buildGrokBuildConfig,
  parseGrokBuildConfig,
  updateGrokBuildConfig,
  validateGrokBuildConfig,
} from "@/utils/grokBuildConfig";
import { resolveProviderIcon } from "@/utils/providerIcon";

type GrokBuildProviderFormProps = Omit<ProviderFormProps, "appId">;

const grokPresetEntries: Array<{
  id: string;
  preset: CodexProviderPreset;
}> = codexProviderPresets
  .map((preset, index) => ({ id: `grokbuild-${index}`, preset }))
  .filter(({ preset }) => preset.category !== "official" && !preset.isOfficial);

export const grokApiBackendFromApiFormat = (format: CodexApiFormat): string => {
  if (format === "openai_chat") return "chat_completions";
  if (format === "anthropic") return "messages";
  return "responses";
};

export function GrokBuildProviderForm({
  providerId,
  submitLabel,
  onSubmit,
  onCancel,
  onSubmittingChange,
  initialData,
  showButtons = true,
}: GrokBuildProviderFormProps) {
  const { t } = useTranslation();
  const isDarkMode = useDarkMode();
  const initialConfigText =
    typeof initialData?.settingsConfig?.config === "string"
      ? initialData.settingsConfig.config
      : undefined;
  const initialConfig = useMemo(
    () => parseGrokBuildConfig(initialConfigText, initialData?.name),
    [initialConfigText, initialData?.name],
  );

  const [selectedPresetId, setSelectedPresetId] = useState<string | null>(
    initialData ? null : "custom",
  );
  const [category, setCategory] = useState<ProviderCategory | undefined>(
    initialData?.category ?? "custom",
  );
  const [isPartner, setIsPartner] = useState(
    initialData?.meta?.isPartner ?? false,
  );
  const [partnerPromotionKey, setPartnerPromotionKey] = useState<string>();
  const [profile, setProfile] = useState(initialConfig.model);
  const [upstreamModel, setUpstreamModel] = useState(
    initialConfig.upstreamModel ?? initialConfig.model,
  );
  const [baseUrl, setBaseUrl] = useState(initialConfig.baseUrl);
  const [apiKey, setApiKey] = useState(initialConfig.apiKey);
  const [apiBackend, setApiBackend] = useState(initialConfig.apiBackend);
  const [contextWindow, setContextWindow] = useState(
    String(initialConfig.contextWindow),
  );
  const [rawConfig, setRawConfig] = useState(
    initialConfigText ?? buildGrokBuildConfig(initialConfig),
  );
  const [apiFormat, setApiFormat] = useState<CodexApiFormat>(
    (initialData?.meta?.apiFormat as CodexApiFormat | undefined) ??
      "openai_responses",
  );
  const [anthropicAuthField, setAnthropicAuthField] =
    useState<ClaudeApiKeyField>(
      initialData?.meta?.apiKeyField ?? "ANTHROPIC_AUTH_TOKEN",
    );
  const [impersonateClaudeCode, setImpersonateClaudeCode] = useState(
    initialData?.meta?.impersonateClaudeCode === true,
  );
  const [maxOutputTokens, setMaxOutputTokens] = useState(
    initialData?.meta?.maxOutputTokens
      ? String(initialData.meta.maxOutputTokens)
      : "",
  );
  const [codexChatReasoning, setCodexChatReasoning] =
    useState<CodexChatReasoning>(initialData?.meta?.codexChatReasoning ?? {});
  const [promptCacheRouting, setPromptCacheRouting] =
    useState<PromptCacheRoutingMode>(
      initialData?.meta?.promptCacheRouting ?? "auto",
    );
  const [isFullUrl, setIsFullUrl] = useState(
    initialData?.meta?.isFullUrl ?? false,
  );
  const [customUserAgent, setCustomUserAgent] = useState(
    initialData?.meta?.customUserAgent ?? "",
  );
  const [headersOverride, setHeadersOverride] = useState(
    formatRequestOverrideObject(
      initialData?.meta?.localProxyRequestOverrides?.headers,
    ),
  );
  const [bodyOverride, setBodyOverride] = useState(
    formatRequestOverrideObject(
      initialData?.meta?.localProxyRequestOverrides?.body,
    ),
  );
  const [endpointAutoSelect, setEndpointAutoSelect] = useState(
    initialData?.meta?.endpointAutoSelect ?? true,
  );
  const [isEndpointModalOpen, setIsEndpointModalOpen] = useState(false);
  const [presetEndpoints, setPresetEndpoints] = useState<string[]>([]);
  const [draftCustomEndpoints, setDraftCustomEndpoints] = useState<string[]>(
    [],
  );

  const form = useForm<ProviderFormData>({
    resolver: zodResolver(providerSchema),
    defaultValues: {
      name: initialData?.name ?? initialConfig.name,
      websiteUrl: initialData?.websiteUrl ?? "",
      notes: initialData?.notes ?? "",
      settingsConfig: JSON.stringify({ config: rawConfig }),
      icon:
        resolveProviderIcon(
          "grokbuild",
          initialData?.icon,
          initialData?.iconColor,
        ) ?? "",
      iconColor: initialData?.iconColor ?? "",
    },
    mode: "onSubmit",
  });
  const { isSubmitting } = form.formState;
  const websiteUrl = form.watch("websiteUrl") ?? "";

  useEffect(() => {
    onSubmittingChange?.(isSubmitting);
  }, [isSubmitting, onSubmittingChange]);

  const presetCategoryLabels = useMemo(
    () => ({
      official: t("providerForm.categoryOfficial", { defaultValue: "官方" }),
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

  const speedTestEndpoints = useMemo(() => {
    const urls = new Set<string>();
    const add = (url?: string) => {
      const normalized = url?.trim().replace(/\/+$/, "");
      if (normalized) urls.add(normalized);
    };
    add(baseUrl);
    presetEndpoints.forEach(add);
    draftCustomEndpoints.forEach(add);
    return Array.from(urls).map((url) => ({ url }));
  }, [baseUrl, draftCustomEndpoints, presetEndpoints]);

  const syncStructuredConfig = (
    overrides: Partial<ReturnType<typeof parseGrokBuildConfig>>,
  ) => {
    const next = {
      model: profile,
      upstreamModel,
      baseUrl,
      name: form.getValues("name") || initialConfig.name,
      apiKey,
      apiBackend,
      contextWindow: Number.parseInt(contextWindow, 10),
      ...overrides,
    };
    setRawConfig((current) => updateGrokBuildConfig(current, next));
  };

  const handlePresetChange = (presetId: string) => {
    setSelectedPresetId(presetId);
    if (presetId === "custom") {
      setCategory("custom");
      setIsPartner(false);
      setPartnerPromotionKey(undefined);
      setPresetEndpoints([]);
      return;
    }

    const entry = grokPresetEntries.find(
      (candidate) => candidate.id === presetId,
    );
    if (!entry) return;
    const preset = entry.preset;
    const presetName = preset.nameKey ? String(t(preset.nameKey)) : preset.name;
    const presetBaseUrl = extractCodexBaseUrl(preset.config) ?? "";
    const presetModel = extractCodexModelName(preset.config) ?? profile;
    const presetApiFormat =
      preset.apiFormat ??
      codexApiFormatFromWireApi(extractCodexWireApi(preset.config)) ??
      "openai_responses";
    const presetApiKey =
      "auth" in preset && typeof preset.auth?.OPENAI_API_KEY === "string"
        ? preset.auth.OPENAI_API_KEY
        : "";
    const presetApiBackend = grokApiBackendFromApiFormat(presetApiFormat);

    form.setValue("name", presetName);
    form.setValue("websiteUrl", preset.websiteUrl ?? "");
    form.setValue("icon", preset.icon ?? "");
    form.setValue("iconColor", preset.iconColor ?? "");
    setCategory(preset.category ?? "custom");
    setIsPartner(preset.isPartner ?? false);
    setPartnerPromotionKey(preset.partnerPromotionKey);
    setBaseUrl(presetBaseUrl);
    setApiKey(presetApiKey);
    setUpstreamModel(presetModel);
    setApiFormat(presetApiFormat);
    setApiBackend(presetApiBackend);
    setPresetEndpoints(preset.endpointCandidates ?? []);
    setRawConfig(
      buildGrokBuildConfig({
        model: profile,
        upstreamModel: presetModel,
        baseUrl: presetBaseUrl,
        name: presetName,
        apiKey: presetApiKey,
        apiBackend: presetApiBackend,
        contextWindow: Number.parseInt(contextWindow, 10),
      }),
    );
  };

  const handleRawConfigChange = (value: string) => {
    setRawConfig(value);
    if (validateGrokBuildConfig(value)) return;
    const parsed = parseGrokBuildConfig(value, form.getValues("name"));
    setProfile(parsed.model);
    setUpstreamModel(parsed.upstreamModel ?? parsed.model);
    setBaseUrl(parsed.baseUrl);
    setApiKey(parsed.apiKey);
    setApiBackend(parsed.apiBackend);
    setContextWindow(String(parsed.contextWindow));
    if (parsed.name) form.setValue("name", parsed.name);
  };

  const handleSubmit = async (values: ProviderFormData) => {
    const name = values.name.trim();
    const parsedContextWindow = Number.parseInt(contextWindow, 10);
    const envKey = parseGrokBuildConfig(rawConfig).envKey?.trim();
    if (
      !name ||
      !baseUrl.trim() ||
      (!apiKey.trim() && !envKey) ||
      !profile.trim()
    ) {
      toast.error(
        t("providerForm.requiredFields", {
          defaultValue: "请填写供应商名称、API 地址、API Key 和模型",
        }),
      );
      return;
    }
    if (!Number.isInteger(parsedContextWindow) || parsedContextWindow <= 0) {
      toast.error(
        t("grokBuild.contextWindowInvalid", {
          defaultValue: "上下文窗口必须是正整数",
        }),
      );
      return;
    }

    const finalConfig = updateGrokBuildConfig(rawConfig, {
      model: profile,
      upstreamModel,
      baseUrl,
      name,
      apiKey,
      apiBackend,
      contextWindow: parsedContextWindow,
    });
    const configError = validateGrokBuildConfig(finalConfig);
    if (configError) {
      toast.error(
        t("grokBuild.invalidToml", {
          error: configError,
          defaultValue: `config.toml 格式错误: ${configError}`,
        }),
      );
      return;
    }

    const requestOverrides = buildLocalProxyRequestOverrides(
      headersOverride,
      bodyOverride,
    );
    if (requestOverrides.error) {
      toast.error(requestOverrides.error);
      return;
    }

    const customEndpoints = Object.fromEntries(
      draftCustomEndpoints.map((url) => [
        url,
        { url, addedAt: Date.now(), lastUsed: undefined },
      ]),
    );
    const parsedMaxOutputTokens = Number.parseInt(maxOutputTokens, 10);
    const initialMeta = { ...(initialData?.meta ?? {}) };
    delete initialMeta.custom_endpoints;
    const meta: ProviderMeta = {
      ...initialMeta,
      apiFormat,
      apiKeyField: anthropicAuthField,
      isFullUrl,
      endpointAutoSelect,
      isPartner,
      partnerPromotionKey,
      impersonateClaudeCode,
      promptCacheRouting,
      codexChatReasoning,
      customUserAgent: customUserAgent.trim() || undefined,
      localProxyRequestOverrides: requestOverrides.overrides,
      maxOutputTokens:
        Number.isInteger(parsedMaxOutputTokens) && parsedMaxOutputTokens > 0
          ? parsedMaxOutputTokens
          : undefined,
    };
    if (!providerId && Object.keys(customEndpoints).length > 0) {
      meta.custom_endpoints = customEndpoints;
    }
    const payload: ProviderFormValues = {
      ...values,
      name,
      websiteUrl: values.websiteUrl?.trim() ?? "",
      notes: values.notes?.trim() ?? "",
      settingsConfig: JSON.stringify({ config: finalConfig }),
      presetId: selectedPresetId ?? undefined,
      presetCategory: category ?? "custom",
      isPartner,
      meta,
    };

    await onSubmit(payload);
  };

  const rawConfigError = validateGrokBuildConfig(rawConfig);

  return (
    <Form {...form}>
      <form
        id="provider-form"
        onSubmit={form.handleSubmit(handleSubmit)}
        className="space-y-6"
      >
        {!initialData && (
          <ProviderPresetSelector
            selectedPresetId={selectedPresetId}
            presetEntries={grokPresetEntries}
            presetCategoryLabels={presetCategoryLabels}
            onPresetChange={handlePresetChange}
            category={category}
          />
        )}

        <BasicFormFields form={form} />

        <div className="grid grid-cols-1 gap-4 md:grid-cols-3">
          <FormItem>
            <FormLabel htmlFor="grokbuild-profile">
              {t("grokBuild.profile", { defaultValue: "客户端模型档位" })}
            </FormLabel>
            <Input
              id="grokbuild-profile"
              value={profile}
              onChange={(event) => {
                const value = event.target.value;
                setProfile(value);
                syncStructuredConfig({ model: value });
              }}
              placeholder="grok-4.5"
              autoComplete="off"
            />
          </FormItem>

          <FormItem>
            <FormLabel htmlFor="grokbuild-api-backend">
              {t("grokBuild.apiBackend", { defaultValue: "API Backend" })}
            </FormLabel>
            <Input
              id="grokbuild-api-backend"
              value={apiBackend}
              onChange={(event) => {
                const value = event.target.value;
                setApiBackend(value);
                syncStructuredConfig({ apiBackend: value });
              }}
              placeholder="responses"
              autoComplete="off"
            />
          </FormItem>

          <FormItem>
            <FormLabel htmlFor="grokbuild-context-window">
              {t("grokBuild.contextWindow", { defaultValue: "上下文窗口" })}
            </FormLabel>
            <Input
              id="grokbuild-context-window"
              type="number"
              min={1}
              step={1}
              value={contextWindow}
              onChange={(event) => {
                const value = event.target.value;
                setContextWindow(value);
                syncStructuredConfig({
                  contextWindow: Number.parseInt(value, 10),
                });
              }}
            />
          </FormItem>
        </div>

        <CodexFormFields
          appId="grokbuild"
          providerId={providerId}
          codexApiKey={apiKey}
          onApiKeyChange={(value) => {
            setApiKey(value);
            syncStructuredConfig({ apiKey: value });
          }}
          category={category}
          shouldShowApiKeyLink={Boolean(websiteUrl)}
          websiteUrl={websiteUrl}
          isPartner={isPartner}
          partnerPromotionKey={partnerPromotionKey}
          shouldShowSpeedTest
          codexBaseUrl={baseUrl}
          onBaseUrlChange={(value) => {
            setBaseUrl(value);
            syncStructuredConfig({ baseUrl: value });
          }}
          isFullUrl={isFullUrl}
          onFullUrlChange={setIsFullUrl}
          isEndpointModalOpen={isEndpointModalOpen}
          onEndpointModalToggle={setIsEndpointModalOpen}
          onCustomEndpointsChange={setDraftCustomEndpoints}
          autoSelect={endpointAutoSelect}
          onAutoSelectChange={setEndpointAutoSelect}
          codexModel={upstreamModel}
          onModelChange={(value) => {
            setUpstreamModel(value);
            syncStructuredConfig({ upstreamModel: value });
          }}
          apiFormat={apiFormat}
          onApiFormatChange={(value) => {
            const backend = grokApiBackendFromApiFormat(value);
            setApiFormat(value);
            setApiBackend(backend);
            syncStructuredConfig({ apiBackend: backend });
          }}
          anthropicAuthField={anthropicAuthField}
          onAnthropicAuthFieldChange={setAnthropicAuthField}
          impersonateClaudeCode={impersonateClaudeCode}
          onImpersonateClaudeCodeChange={setImpersonateClaudeCode}
          maxOutputTokens={maxOutputTokens}
          onMaxOutputTokensChange={setMaxOutputTokens}
          codexChatReasoning={codexChatReasoning}
          onCodexChatReasoningChange={setCodexChatReasoning}
          promptCacheRouting={promptCacheRouting}
          onPromptCacheRoutingChange={setPromptCacheRouting}
          speedTestEndpoints={speedTestEndpoints}
          customUserAgent={customUserAgent}
          onCustomUserAgentChange={setCustomUserAgent}
          localProxyHeadersOverride={headersOverride}
          onLocalProxyHeadersOverrideChange={setHeadersOverride}
          localProxyBodyOverride={bodyOverride}
          onLocalProxyBodyOverrideChange={setBodyOverride}
        />

        <div className="space-y-2">
          <FormLabel htmlFor="grokbuild-config-toml">
            {t("grokBuild.rawConfig", { defaultValue: "config.toml" })}
          </FormLabel>
          <JsonEditor
            value={rawConfig}
            onChange={handleRawConfigChange}
            placeholder=""
            darkMode={isDarkMode}
            rows={12}
            showValidation={false}
            language="javascript"
          />
          {rawConfigError && (
            <p className="text-xs text-destructive">
              {t("grokBuild.invalidToml", {
                error: rawConfigError,
                defaultValue: `Invalid config.toml: ${rawConfigError}`,
              })}
            </p>
          )}
        </div>

        <FormField
          control={form.control}
          name="settingsConfig"
          render={() => (
            <FormItem className="hidden">
              <FormControl>
                <Input type="hidden" />
              </FormControl>
              <FormMessage />
            </FormItem>
          )}
        />

        {showButtons && (
          <div className="flex justify-end gap-2">
            <Button variant="outline" type="button" onClick={onCancel}>
              {t("common.cancel")}
            </Button>
            <Button type="submit" disabled={isSubmitting}>
              {submitLabel}
            </Button>
          </div>
        )}
      </form>
    </Form>
  );
}
