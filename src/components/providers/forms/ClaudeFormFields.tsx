import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { FormLabel } from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Button } from "@/components/ui/button";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from "@/components/ui/command";
import { Check, ChevronDown, RefreshCw } from "lucide-react";
import { cn } from "@/lib/utils";
import { providersApi, type RemoteModelInfo } from "@/lib/api/providers";
import EndpointSpeedTest from "./EndpointSpeedTest";
import { ApiKeySection, EndpointField } from "./shared";
import type {
  ProviderCategory,
  ClaudeApiFormat,
  ProviderProxyConfig,
} from "@/types";
import type { TemplateValueConfig } from "@/config/claudeProviderPresets";

interface EndpointCandidate {
  url: string;
}

interface ClaudeFormFieldsProps {
  providerId?: string;
  // API Key
  shouldShowApiKey: boolean;
  apiKey: string;
  onApiKeyChange: (key: string) => void;
  category?: ProviderCategory;
  shouldShowApiKeyLink: boolean;
  websiteUrl: string;
  isPartner?: boolean;
  partnerPromotionKey?: string;

  // Template Values
  templateValueEntries: Array<[string, TemplateValueConfig]>;
  templateValues: Record<string, TemplateValueConfig>;
  templatePresetName: string;
  onTemplateValueChange: (key: string, value: string) => void;

  // Base URL
  shouldShowSpeedTest: boolean;
  baseUrl: string;
  onBaseUrlChange: (url: string) => void;
  isEndpointModalOpen: boolean;
  onEndpointModalToggle: (open: boolean) => void;
  onCustomEndpointsChange?: (endpoints: string[]) => void;
  autoSelect: boolean;
  onAutoSelectChange: (checked: boolean) => void;

  // Model Selector
  shouldShowModelSelector: boolean;
  claudeModel: string;
  reasoningModel: string;
  defaultHaikuModel: string;
  defaultSonnetModel: string;
  defaultOpusModel: string;
  onModelChange: (
    field:
      | "ANTHROPIC_MODEL"
      | "ANTHROPIC_REASONING_MODEL"
      | "ANTHROPIC_DEFAULT_HAIKU_MODEL"
      | "ANTHROPIC_DEFAULT_SONNET_MODEL"
      | "ANTHROPIC_DEFAULT_OPUS_MODEL",
    value: string,
  ) => void;

  // Speed Test Endpoints
  speedTestEndpoints: EndpointCandidate[];

  // API Format (for third-party providers that use OpenAI Chat Completions format)
  apiFormat: ClaudeApiFormat;
  onApiFormatChange: (format: ClaudeApiFormat) => void;

  // Provider-level proxy config for model enumeration
  proxyConfig?: ProviderProxyConfig;
}

interface RemoteModelSelectorProps {
  id: string;
  label: string;
  value: string;
  placeholder?: string;
  models: RemoteModelInfo[];
  isLoading: boolean;
  onChange: (value: string) => void;
  onOpen: () => void;
  onRefresh: () => void;
}

const REMOTE_MODELS_CACHE_TTL_MS = 5 * 60 * 1000;
const remoteModelsCache = new Map<
  string,
  { expiresAt: number; models: RemoteModelInfo[] }
>();
const remoteModelsInflight = new Map<string, Promise<RemoteModelInfo[]>>();

function normalizeBaseUrl(baseUrl: string): string {
  return baseUrl.trim().replace(/\/+$/, "");
}

function apiKeyFingerprint(apiKey: string): string {
  let hash = 0;
  for (let i = 0; i < apiKey.length; i += 1) {
    hash = (hash * 31 + apiKey.charCodeAt(i)) >>> 0;
  }
  return hash.toString(16);
}

function normalizeProviderName(model: RemoteModelInfo): string | undefined {
  const provider =
    typeof model.provider === "string" ? model.provider.trim() : "";
  return provider || undefined;
}

function normalizeRemoteModels(models: RemoteModelInfo[]): RemoteModelInfo[] {
  const seen = new Set<string>();
  const normalized: RemoteModelInfo[] = [];

  for (const model of models) {
    const id = typeof model.id === "string" ? model.id.trim() : "";
    if (!id || seen.has(id)) continue;
    seen.add(id);
    normalized.push({
      id,
      provider:
        typeof model.provider === "string" ? model.provider.trim() : undefined,
      displayName:
        typeof model.displayName === "string"
          ? model.displayName.trim()
          : undefined,
    });
  }

  return normalized.sort((a, b) =>
    a.id.localeCompare(b.id, "en", { numeric: true }),
  );
}

function RemoteModelSelector({
  id,
  label,
  value,
  placeholder,
  models,
  isLoading,
  onChange,
  onOpen,
  onRefresh,
}: RemoteModelSelectorProps) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [search, setSearch] = useState("");

  const searchText = search.trim();
  const hasExactModel = models.some(
    (model) => model.id.toLowerCase() === searchText.toLowerCase(),
  );

  return (
    <div className="space-y-2">
      <FormLabel htmlFor={id}>{label}</FormLabel>
      <div className="flex items-center gap-2">
        <Input
          id={id}
          type="text"
          value={value}
          onChange={(e) => onChange(e.target.value)}
          placeholder={placeholder}
          autoComplete="off"
        />

        <Popover
          modal
          open={open}
          onOpenChange={(next) => {
            setOpen(next);
            if (next) {
              onOpen();
            } else {
              setSearch("");
            }
          }}
        >
          <PopoverTrigger asChild>
            <Button
              type="button"
              variant="outline"
              size="icon"
              className="h-9 w-9 shrink-0"
              aria-label={t("providerForm.selectModel", {
                defaultValue: "Select model",
              })}
            >
              <ChevronDown className="h-4 w-4" />
            </Button>
          </PopoverTrigger>
          <PopoverContent
            side="bottom"
            align="end"
            sideOffset={6}
            avoidCollisions={true}
            collisionPadding={8}
            className="z-[1000] w-[30rem] max-w-[calc(100vw-2rem)] p-0 border-border-default"
          >
            <Command>
              <CommandInput
                value={search}
                onValueChange={setSearch}
                placeholder={t("providerForm.searchModels", {
                  defaultValue: "Search models...",
                })}
              />
              <CommandList>
                <CommandEmpty>
                  {isLoading
                    ? t("providerForm.loadingModels", {
                        defaultValue: "Loading models...",
                      })
                    : t("providerForm.noModelsFound", {
                        defaultValue: "No models found",
                      })}
                </CommandEmpty>

                {searchText && !hasExactModel && (
                  <CommandGroup
                    heading={t("providerForm.customModel", {
                      defaultValue: "Custom",
                    })}
                  >
                    <CommandItem
                      value={`custom:${searchText}`}
                      onSelect={() => {
                        onChange(searchText);
                        setOpen(false);
                        setSearch("");
                      }}
                    >
                      <Check className="mr-2 h-4 w-4 opacity-0" />
                      <div className="min-w-0 flex flex-col">
                        <span className="truncate">{searchText}</span>
                        <span className="truncate text-xs text-muted-foreground">
                          {t("providerForm.useTypedModel", {
                            defaultValue: "Use typed model",
                          })}
                        </span>
                      </div>
                    </CommandItem>
                  </CommandGroup>
                )}

                <CommandGroup>
                  {models.map((model) => {
                    const provider = normalizeProviderName(model);
                    return (
                      <CommandItem
                        key={model.id}
                        value={`${model.id} ${provider ?? ""}`}
                        keywords={[
                          model.id,
                          provider ?? "",
                          model.displayName ?? "",
                        ]}
                        onSelect={() => {
                          onChange(model.id);
                          setOpen(false);
                          setSearch("");
                        }}
                      >
                        <Check
                          className={cn(
                            "mr-2 h-4 w-4",
                            value === model.id ? "opacity-100" : "opacity-0",
                          )}
                        />
                        <div className="min-w-0 flex flex-col leading-tight">
                          <span className="truncate">{model.id}</span>
                          {provider && (
                            <span className="truncate text-xs text-muted-foreground">
                              {provider}
                            </span>
                          )}
                        </div>
                      </CommandItem>
                    );
                  })}
                </CommandGroup>
              </CommandList>
            </Command>
          </PopoverContent>
        </Popover>

        <Button
          type="button"
          variant="outline"
          size="icon"
          className="h-9 w-9 shrink-0"
          onClick={onRefresh}
          disabled={isLoading}
        >
          <RefreshCw className={cn("h-4 w-4", isLoading && "animate-spin")} />
        </Button>
      </div>
    </div>
  );
}

export function ClaudeFormFields({
  providerId,
  shouldShowApiKey,
  apiKey,
  onApiKeyChange,
  category,
  shouldShowApiKeyLink,
  websiteUrl,
  isPartner,
  partnerPromotionKey,
  templateValueEntries,
  templateValues,
  templatePresetName,
  onTemplateValueChange,
  shouldShowSpeedTest,
  baseUrl,
  onBaseUrlChange,
  isEndpointModalOpen,
  onEndpointModalToggle,
  onCustomEndpointsChange,
  autoSelect,
  onAutoSelectChange,
  shouldShowModelSelector,
  claudeModel,
  reasoningModel,
  defaultHaikuModel,
  defaultSonnetModel,
  defaultOpusModel,
  onModelChange,
  speedTestEndpoints,
  apiFormat,
  onApiFormatChange,
  proxyConfig,
}: ClaudeFormFieldsProps) {
  const { t } = useTranslation();

  const canEnumerateRemoteModels =
    shouldShowModelSelector &&
    category !== "official" &&
    normalizeBaseUrl(baseUrl).length > 0 &&
    apiKey.trim().length > 0;

  const cacheKey = useMemo(() => {
    const normalizedBaseUrl = normalizeBaseUrl(baseUrl);
    if (!normalizedBaseUrl) return "";

    const proxyFingerprint = proxyConfig?.enabled
      ? `${proxyConfig.proxyType ?? "http"}:${proxyConfig.proxyHost ?? ""}:${proxyConfig.proxyPort ?? ""}`
      : "none";

    return `${apiFormat}|${normalizedBaseUrl}|${apiKeyFingerprint(apiKey.trim())}|${proxyFingerprint}`;
  }, [
    baseUrl,
    apiFormat,
    apiKey,
    proxyConfig?.enabled,
    proxyConfig?.proxyType,
    proxyConfig?.proxyHost,
    proxyConfig?.proxyPort,
  ]);

  const [remoteModels, setRemoteModels] = useState<RemoteModelInfo[]>([]);
  const [isLoadingRemoteModels, setIsLoadingRemoteModels] = useState(false);
  const [hasLoadedRemoteModels, setHasLoadedRemoteModels] = useState(false);
  const [remoteModelsError, setRemoteModelsError] = useState<string | null>(
    null,
  );

  useEffect(() => {
    if (!canEnumerateRemoteModels || !cacheKey) {
      setRemoteModels([]);
      setHasLoadedRemoteModels(false);
      setRemoteModelsError(null);
      return;
    }

    const cached = remoteModelsCache.get(cacheKey);
    if (cached && cached.expiresAt > Date.now()) {
      setRemoteModels(cached.models);
      setHasLoadedRemoteModels(true);
      setRemoteModelsError(null);
      return;
    }

    setRemoteModels([]);
    setHasLoadedRemoteModels(false);
    setRemoteModelsError(null);
  }, [canEnumerateRemoteModels, cacheKey]);

  const loadRemoteModels = useCallback(
    async (forceRefresh = false, notifyOnError = false) => {
      if (!canEnumerateRemoteModels || !cacheKey) return;

      if (!forceRefresh) {
        const cached = remoteModelsCache.get(cacheKey);
        if (cached && cached.expiresAt > Date.now()) {
          setRemoteModels(cached.models);
          setHasLoadedRemoteModels(true);
          setRemoteModelsError(null);
          return;
        }
      }

      setIsLoadingRemoteModels(true);
      setRemoteModelsError(null);

      let request = !forceRefresh
        ? remoteModelsInflight.get(cacheKey)
        : undefined;
      if (!request) {
        request = providersApi
          .enumerateModels({
            baseUrl: normalizeBaseUrl(baseUrl),
            apiKey: apiKey.trim(),
            apiFormat,
            proxyConfig: proxyConfig?.enabled ? proxyConfig : undefined,
            forceRefresh,
          })
          .then((models) => normalizeRemoteModels(models));
        remoteModelsInflight.set(cacheKey, request);
      }

      try {
        const models = await request;
        remoteModelsCache.set(cacheKey, {
          expiresAt: Date.now() + REMOTE_MODELS_CACHE_TTL_MS,
          models,
        });
        setRemoteModels(models);
        setHasLoadedRemoteModels(true);
        setRemoteModelsError(null);
      } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        setRemoteModelsError(message);
        if (notifyOnError) {
          toast.error(
            t("providerForm.fetchModelsFailed", {
              defaultValue: "Failed to fetch models: {{error}}",
              error: message,
            }),
          );
        }
      } finally {
        if (remoteModelsInflight.get(cacheKey) === request) {
          remoteModelsInflight.delete(cacheKey);
        }
        setIsLoadingRemoteModels(false);
      }
    },
    [
      canEnumerateRemoteModels,
      cacheKey,
      baseUrl,
      apiKey,
      apiFormat,
      proxyConfig,
      t,
    ],
  );

  const handleModelSelectorOpen = useCallback(() => {
    if (!hasLoadedRemoteModels && !isLoadingRemoteModels) {
      void loadRemoteModels(false, false);
    }
  }, [hasLoadedRemoteModels, isLoadingRemoteModels, loadRemoteModels]);

  const handleRefreshModels = useCallback(() => {
    void loadRemoteModels(true, true);
  }, [loadRemoteModels]);

  return (
    <>
      {/* API Key 输入框 */}
      {shouldShowApiKey && (
        <ApiKeySection
          value={apiKey}
          onChange={onApiKeyChange}
          category={category}
          shouldShowLink={shouldShowApiKeyLink}
          websiteUrl={websiteUrl}
          isPartner={isPartner}
          partnerPromotionKey={partnerPromotionKey}
        />
      )}

      {/* 模板变量输入 */}
      {templateValueEntries.length > 0 && (
        <div className="space-y-3">
          <FormLabel>
            {t("providerForm.parameterConfig", {
              name: templatePresetName,
              defaultValue: `${templatePresetName} 参数配置`,
            })}
          </FormLabel>
          <div className="space-y-4">
            {templateValueEntries.map(([key, config]) => (
              <div key={key} className="space-y-2">
                <FormLabel htmlFor={`template-${key}`}>
                  {config.label}
                </FormLabel>
                <Input
                  id={`template-${key}`}
                  type="text"
                  required
                  value={
                    templateValues[key]?.editorValue ??
                    config.editorValue ??
                    config.defaultValue ??
                    ""
                  }
                  onChange={(e) => onTemplateValueChange(key, e.target.value)}
                  placeholder={config.placeholder || config.label}
                  autoComplete="off"
                />
              </div>
            ))}
          </div>
        </div>
      )}

      {/* Base URL 输入框 */}
      {shouldShowSpeedTest && (
        <EndpointField
          id="baseUrl"
          label={t("providerForm.apiEndpoint")}
          value={baseUrl}
          onChange={onBaseUrlChange}
          placeholder={t("providerForm.apiEndpointPlaceholder")}
          hint={
            apiFormat === "openai_chat"
              ? t("providerForm.apiHintOAI")
              : t("providerForm.apiHint")
          }
          onManageClick={() => onEndpointModalToggle(true)}
        />
      )}

      {/* 端点测速弹窗 */}
      {shouldShowSpeedTest && isEndpointModalOpen && (
        <EndpointSpeedTest
          appId="claude"
          providerId={providerId}
          value={baseUrl}
          onChange={onBaseUrlChange}
          initialEndpoints={speedTestEndpoints}
          visible={isEndpointModalOpen}
          onClose={() => onEndpointModalToggle(false)}
          autoSelect={autoSelect}
          onAutoSelectChange={onAutoSelectChange}
          onCustomEndpointsChange={onCustomEndpointsChange}
        />
      )}

      {/* API 格式选择（仅非官方供应商显示） */}
      {shouldShowModelSelector && (
        <div className="space-y-2">
          <FormLabel htmlFor="apiFormat">
            {t("providerForm.apiFormat", { defaultValue: "API 格式" })}
          </FormLabel>
          <Select value={apiFormat} onValueChange={onApiFormatChange}>
            <SelectTrigger id="apiFormat" className="w-full">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="anthropic">
                {t("providerForm.apiFormatAnthropic", {
                  defaultValue: "Anthropic Messages (原生)",
                })}
              </SelectItem>
              <SelectItem value="openai_chat">
                {t("providerForm.apiFormatOpenAIChat", {
                  defaultValue: "OpenAI Chat Completions (需转换)",
                })}
              </SelectItem>
            </SelectContent>
          </Select>
          <p className="text-xs text-muted-foreground">
            {t("providerForm.apiFormatHint", {
              defaultValue: "选择供应商 API 的输入格式",
            })}
          </p>
        </div>
      )}

      {/* 模型选择器 */}
      {shouldShowModelSelector && (
        <div className="space-y-3">
          <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
            {canEnumerateRemoteModels ? (
              <>
                <RemoteModelSelector
                  id="claudeModel"
                  label={t("providerForm.anthropicModel", {
                    defaultValue: "主模型",
                  })}
                  value={claudeModel}
                  placeholder={t("providerForm.modelPlaceholder", {
                    defaultValue: "",
                  })}
                  models={remoteModels}
                  isLoading={isLoadingRemoteModels}
                  onOpen={handleModelSelectorOpen}
                  onRefresh={handleRefreshModels}
                  onChange={(value) => onModelChange("ANTHROPIC_MODEL", value)}
                />

                <RemoteModelSelector
                  id="reasoningModel"
                  label={t("providerForm.anthropicReasoningModel", {
                    defaultValue: "推理模型",
                  })}
                  value={reasoningModel}
                  models={remoteModels}
                  isLoading={isLoadingRemoteModels}
                  onOpen={handleModelSelectorOpen}
                  onRefresh={handleRefreshModels}
                  onChange={(value) =>
                    onModelChange("ANTHROPIC_REASONING_MODEL", value)
                  }
                />

                <RemoteModelSelector
                  id="claudeDefaultHaikuModel"
                  label={t("providerForm.anthropicDefaultHaikuModel", {
                    defaultValue: "Haiku 默认模型",
                  })}
                  value={defaultHaikuModel}
                  placeholder={t("providerForm.haikuModelPlaceholder", {
                    defaultValue: "",
                  })}
                  models={remoteModels}
                  isLoading={isLoadingRemoteModels}
                  onOpen={handleModelSelectorOpen}
                  onRefresh={handleRefreshModels}
                  onChange={(value) =>
                    onModelChange("ANTHROPIC_DEFAULT_HAIKU_MODEL", value)
                  }
                />

                <RemoteModelSelector
                  id="claudeDefaultSonnetModel"
                  label={t("providerForm.anthropicDefaultSonnetModel", {
                    defaultValue: "Sonnet 默认模型",
                  })}
                  value={defaultSonnetModel}
                  placeholder={t("providerForm.modelPlaceholder", {
                    defaultValue: "",
                  })}
                  models={remoteModels}
                  isLoading={isLoadingRemoteModels}
                  onOpen={handleModelSelectorOpen}
                  onRefresh={handleRefreshModels}
                  onChange={(value) =>
                    onModelChange("ANTHROPIC_DEFAULT_SONNET_MODEL", value)
                  }
                />

                <RemoteModelSelector
                  id="claudeDefaultOpusModel"
                  label={t("providerForm.anthropicDefaultOpusModel", {
                    defaultValue: "Opus 默认模型",
                  })}
                  value={defaultOpusModel}
                  placeholder={t("providerForm.modelPlaceholder", {
                    defaultValue: "",
                  })}
                  models={remoteModels}
                  isLoading={isLoadingRemoteModels}
                  onOpen={handleModelSelectorOpen}
                  onRefresh={handleRefreshModels}
                  onChange={(value) =>
                    onModelChange("ANTHROPIC_DEFAULT_OPUS_MODEL", value)
                  }
                />
              </>
            ) : (
              <>
                {/* 主模型 */}
                <div className="space-y-2">
                  <FormLabel htmlFor="claudeModel">
                    {t("providerForm.anthropicModel", {
                      defaultValue: "主模型",
                    })}
                  </FormLabel>
                  <Input
                    id="claudeModel"
                    type="text"
                    value={claudeModel}
                    onChange={(e) =>
                      onModelChange("ANTHROPIC_MODEL", e.target.value)
                    }
                    placeholder={t("providerForm.modelPlaceholder", {
                      defaultValue: "",
                    })}
                    autoComplete="off"
                  />
                </div>

                {/* 推理模型 */}
                <div className="space-y-2">
                  <FormLabel htmlFor="reasoningModel">
                    {t("providerForm.anthropicReasoningModel")}
                  </FormLabel>
                  <Input
                    id="reasoningModel"
                    type="text"
                    value={reasoningModel}
                    onChange={(e) =>
                      onModelChange("ANTHROPIC_REASONING_MODEL", e.target.value)
                    }
                    autoComplete="off"
                  />
                </div>

                {/* 默认 Haiku */}
                <div className="space-y-2">
                  <FormLabel htmlFor="claudeDefaultHaikuModel">
                    {t("providerForm.anthropicDefaultHaikuModel", {
                      defaultValue: "Haiku 默认模型",
                    })}
                  </FormLabel>
                  <Input
                    id="claudeDefaultHaikuModel"
                    type="text"
                    value={defaultHaikuModel}
                    onChange={(e) =>
                      onModelChange(
                        "ANTHROPIC_DEFAULT_HAIKU_MODEL",
                        e.target.value,
                      )
                    }
                    placeholder={t("providerForm.haikuModelPlaceholder", {
                      defaultValue: "",
                    })}
                    autoComplete="off"
                  />
                </div>

                {/* 默认 Sonnet */}
                <div className="space-y-2">
                  <FormLabel htmlFor="claudeDefaultSonnetModel">
                    {t("providerForm.anthropicDefaultSonnetModel", {
                      defaultValue: "Sonnet 默认模型",
                    })}
                  </FormLabel>
                  <Input
                    id="claudeDefaultSonnetModel"
                    type="text"
                    value={defaultSonnetModel}
                    onChange={(e) =>
                      onModelChange(
                        "ANTHROPIC_DEFAULT_SONNET_MODEL",
                        e.target.value,
                      )
                    }
                    placeholder={t("providerForm.modelPlaceholder", {
                      defaultValue: "",
                    })}
                    autoComplete="off"
                  />
                </div>

                {/* 默认 Opus */}
                <div className="space-y-2">
                  <FormLabel htmlFor="claudeDefaultOpusModel">
                    {t("providerForm.anthropicDefaultOpusModel", {
                      defaultValue: "Opus 默认模型",
                    })}
                  </FormLabel>
                  <Input
                    id="claudeDefaultOpusModel"
                    type="text"
                    value={defaultOpusModel}
                    onChange={(e) =>
                      onModelChange(
                        "ANTHROPIC_DEFAULT_OPUS_MODEL",
                        e.target.value,
                      )
                    }
                    placeholder={t("providerForm.modelPlaceholder", {
                      defaultValue: "",
                    })}
                    autoComplete="off"
                  />
                </div>
              </>
            )}
          </div>

          {canEnumerateRemoteModels && remoteModelsError && (
            <p className="text-xs text-destructive">{remoteModelsError}</p>
          )}

          <p className="text-xs text-muted-foreground">
            {t("providerForm.modelHelper", {
              defaultValue:
                "可选：指定默认使用的 Claude 模型，留空则使用系统默认。",
            })}
          </p>
        </div>
      )}
    </>
  );
}
