import { useTranslation } from "react-i18next";
import {
  useState,
  useRef,
  useCallback,
  useMemo,
  useEffect,
  type ReactNode,
} from "react";
import { FormLabel } from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import { toast } from "sonner";
import {
  Download,
  Plus,
  Trash2,
  ChevronDown,
  ChevronRight,
  Loader2,
} from "lucide-react";
import { ApiKeySection } from "./shared";
import {
  fetchModelsForConfig,
  showFetchModelsError,
  type FetchedModel,
} from "@/lib/api/model-fetch";
import { ModelDropdown } from "./shared/ModelDropdown";
import {
  kimiApiTypes,
  type KimiApiType,
  type KimiModel,
} from "@/config/kimiProviderPresets";
import type { ProviderCategory } from "@/types";

interface KimiFormFieldsProps {
  baseUrl: string;
  onBaseUrlChange: (value: string) => void;
  apiKey: string;
  onApiKeyChange: (value: string) => void;
  category?: ProviderCategory;
  shouldShowApiKeyLink: boolean;
  websiteUrl: string;
  isPartner?: boolean;
  partnerPromotionKey?: string;
  type: KimiApiType;
  onTypeChange: (type: KimiApiType) => void;
  models: KimiModel[];
  onModelsChange: (models: KimiModel[]) => void;
  defaultModel: string;
  onDefaultModelChange: (alias: string) => void;
}

type BaseUrlErrorCode = "empty" | "invalid" | "scheme";

const BASE_URL_ERROR_I18N_KEY: Record<BaseUrlErrorCode, string> = {
  empty: "kimi.form.baseUrlRequired",
  scheme: "kimi.form.baseUrlScheme",
  invalid: "kimi.form.baseUrlInvalid",
};

const TEMPLATE_TOKEN_RE = /\$\{[^}]+\}/g;

/** 上下文长度预设（大部分模型为 1M）。选中 custom 时显示自由输入框。 */
const CONTEXT_SIZE_PRESETS: Array<{ value: number; label: string }> = [
  { value: 1048576, label: "1M (1048576)" },
  { value: 524288, label: "512K (524288)" },
  { value: 262144, label: "256K (262144)" },
  { value: 131072, label: "128K (131072)" },
  { value: 65536, label: "64K (65536)" },
  { value: 32768, label: "32K (32768)" },
];

const CUSTOM_OPTION = "custom";

function isPresetContextSize(v?: number): boolean {
  return v !== undefined && CONTEXT_SIZE_PRESETS.some((p) => p.value === v);
}

/** 推理强度预设 */
const EFFORT_PRESETS = ["low", "medium", "high", "xhigh", "max"];

function validateBaseUrl(raw: string): BaseUrlErrorCode | null {
  const trimmed = raw.trim();
  if (!trimmed) return "empty";
  // Presets may embed `${VAR}` tokens — swap them before URL parse.
  const candidate = trimmed.replace(TEMPLATE_TOKEN_RE, "placeholder");
  let u: URL;
  try {
    u = new URL(candidate);
  } catch {
    return "invalid";
  }
  if (!u.protocol.startsWith("http")) return "scheme";
  if (!u.hostname) return "invalid";
  return null;
}

interface AdvancedSectionProps {
  open: boolean;
  onOpenChange: (next: boolean) => void;
  labelKey: string;
  children: ReactNode;
}

function AdvancedSection({
  open,
  onOpenChange,
  labelKey,
  children,
}: AdvancedSectionProps) {
  const { t } = useTranslation();
  return (
    <Collapsible open={open} onOpenChange={onOpenChange}>
      <CollapsibleTrigger asChild>
        <Button
          type="button"
          variant="ghost"
          size="sm"
          className="h-7 gap-1 text-xs text-muted-foreground hover:text-foreground"
        >
          {open ? (
            <ChevronDown className="h-3.5 w-3.5" />
          ) : (
            <ChevronRight className="h-3.5 w-3.5" />
          )}
          {t(labelKey)}
        </Button>
      </CollapsibleTrigger>
      <CollapsibleContent className="space-y-3 pt-2">
        {children}
      </CollapsibleContent>
    </Collapsible>
  );
}

export function KimiFormFields({
  baseUrl,
  onBaseUrlChange,
  apiKey,
  onApiKeyChange,
  category,
  shouldShowApiKeyLink,
  websiteUrl,
  isPartner,
  partnerPromotionKey,
  type,
  onTypeChange,
  models,
  onModelsChange,
  defaultModel,
  onDefaultModelChange,
}: KimiFormFieldsProps) {
  const { t } = useTranslation();
  const [expandedModels, setExpandedModels] = useState<Record<number, boolean>>(
    {},
  );
  const [fetchedModels, setFetchedModels] = useState<FetchedModel[]>([]);
  const [isFetchingModels, setIsFetchingModels] = useState(false);
  const [baseUrlTouched, setBaseUrlTouched] = useState(false);

  // Auto-expand the capabilities row when a preset brings one in.
  useEffect(() => {
    const anyCapabilities = models.some(
      (m) => m.capabilities && m.capabilities.length > 0,
    );
    if (anyCapabilities) {
      setExpandedModels((prev) =>
        Object.fromEntries(
          models.map((m, i) => [i, prev[i] ?? (m.capabilities?.length ? true : false)]),
        ),
      );
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const baseUrlErrorCode = useMemo(() => validateBaseUrl(baseUrl), [baseUrl]);
  const showBaseUrlError = baseUrlTouched && baseUrlErrorCode !== null;
  const baseUrlErrorMessage = baseUrlErrorCode
    ? t(BASE_URL_ERROR_I18N_KEY[baseUrlErrorCode])
    : "";

  // Stable list keys: a manual ref rather than UUID-in-state so adding/removing
  // rows doesn't re-mount unrelated inputs (would drop focus mid-typing).
  const modelKeysRef = useRef<string[]>([]);
  while (modelKeysRef.current.length < models.length) {
    modelKeysRef.current.push(crypto.randomUUID());
  }
  if (modelKeysRef.current.length > models.length) {
    modelKeysRef.current.length = models.length;
  }
  const modelKeys = modelKeysRef.current;

  const toggleModelAdvanced = (index: number) => {
    setExpandedModels((prev) => ({ ...prev, [index]: !prev[index] }));
  };

  const handleAddModel = () => {
    modelKeysRef.current.push(crypto.randomUUID());
    onModelsChange([
      ...models,
      { id: "", name: "", max_context_size: undefined, capabilities: [] },
    ]);
  };

  const handleFetchModels = useCallback(() => {
    if (!baseUrl || !apiKey) {
      showFetchModelsError(null, t, {
        hasApiKey: !!apiKey,
        hasBaseUrl: !!baseUrl,
      });
      return;
    }
    setIsFetchingModels(true);
    fetchModelsForConfig(baseUrl, apiKey)
      .then((fetched) => {
        setFetchedModels(fetched);
        if (fetched.length === 0) {
          toast.info(t("providerForm.fetchModelsEmpty"));
        } else {
          toast.success(
            t("providerForm.fetchModelsSuccess", { count: fetched.length }),
          );
        }
      })
      .catch((err) => {
        console.warn("[ModelFetch] Failed:", err);
        showFetchModelsError(err, t);
      })
      .finally(() => setIsFetchingModels(false));
  }, [baseUrl, apiKey, t]);

  const handleRemoveModel = (index: number) => {
    modelKeysRef.current.splice(index, 1);
    const next = [...models];
    next.splice(index, 1);
    onModelsChange(next);
    setExpandedModels((prev) => {
      const updated = { ...prev };
      delete updated[index];
      return updated;
    });
  };

  const handleModelChange = (
    index: number,
    field: keyof KimiModel,
    value: unknown,
  ) => {
    const next = [...models];
    next[index] = { ...next[index], [field]: value };
    onModelsChange(next);
  };

  return (
    <>
      <div className="space-y-2">
        <FormLabel htmlFor="kimi-type">
          {t("kimi.form.type", { defaultValue: "协议类型" })}
        </FormLabel>
        <Select
          value={type}
          onValueChange={(v) => onTypeChange(v as KimiApiType)}
        >
          <SelectTrigger id="kimi-type">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {kimiApiTypes.map((item) => (
              <SelectItem key={item.value} value={item.value}>
                {t(item.labelKey)}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <p className="text-xs text-muted-foreground">
          {t("kimi.form.typeHint", {
            defaultValue: "供应商 API 协议（写入 [providers.<name>].type）。",
          })}
        </p>
      </div>

      <div className="space-y-2">
        <FormLabel htmlFor="kimi-baseurl">
          {t("kimi.form.baseUrl", { defaultValue: "API 端点" })}
        </FormLabel>
        <Input
          id="kimi-baseurl"
          value={baseUrl}
          onChange={(e) => onBaseUrlChange(e.target.value)}
          onBlur={() => setBaseUrlTouched(true)}
          placeholder="https://api.moonshot.cn/v1"
          aria-invalid={showBaseUrlError}
          className={
            showBaseUrlError
              ? "border-destructive focus-visible:ring-destructive"
              : undefined
          }
        />
        {showBaseUrlError ? (
          <p className="text-xs text-destructive">{baseUrlErrorMessage}</p>
        ) : (
          <p className="text-xs text-muted-foreground">
            {t("kimi.form.baseUrlHint", {
              defaultValue: "供应商的 API 端点地址。",
            })}
          </p>
        )}
      </div>

      <ApiKeySection
        value={apiKey}
        onChange={onApiKeyChange}
        category={category === "official" ? undefined : category}
        shouldShowLink={shouldShowApiKeyLink}
        websiteUrl={websiteUrl}
        isPartner={isPartner}
        partnerPromotionKey={partnerPromotionKey}
      />

      <div className="space-y-3">
        <div className="flex items-center justify-between gap-2">
          <FormLabel className="shrink-0">
            {t("kimi.form.models", { defaultValue: "模型列表" })}
          </FormLabel>
          <div className="flex flex-wrap items-center gap-1.5">
            {models.length > 0 && (
              <div className="flex items-center gap-1.5">
                <FormLabel className="text-xs text-muted-foreground font-normal">
                  {t("kimi.form.defaultModel", { defaultValue: "默认模型" })}
                </FormLabel>
                <Select
                  value={defaultModel}
                  onValueChange={(v) => onDefaultModelChange(v)}
                >
                  <SelectTrigger className="h-7 w-44 text-xs">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {/* Radix SelectItem 不允许空 value：过滤未填别名的模型行 */}
                    {models
                      .filter((m) => m.id.trim() !== "")
                      .map((m) => (
                        <SelectItem key={m.id} value={m.id}>
                          {m.name || m.id}
                        </SelectItem>
                      ))}
                  </SelectContent>
                </Select>
              </div>
            )}
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
              type="button"
              variant="outline"
              size="sm"
              onClick={handleAddModel}
              className="h-7 gap-1"
            >
              <Plus className="h-3.5 w-3.5" />
              {t("kimi.form.addModel", { defaultValue: "添加模型" })}
            </Button>
          </div>
        </div>

        {models.length === 0 ? (
          <p className="text-sm text-muted-foreground py-2">
            {t("kimi.form.noModels", {
              defaultValue: "暂无模型配置。切换到此供应商时将无默认模型。",
            })}
          </p>
        ) : (
          <div className="space-y-4">
            {models.map((model, index) => {
              const isDefault = defaultModel === model.id;
              return (
                <div
                  key={modelKeys[index]}
                  className={`p-3 rounded-lg space-y-3 border ${
                    isDefault
                      ? "border-blue-500/40 bg-blue-500/[0.04]"
                      : "border-border/50"
                  }`}
                >
                  <div className="flex items-center gap-2">
                    <span
                      className={`text-[11px] font-semibold px-2 py-0.5 rounded-md ${
                        isDefault
                          ? "bg-blue-500 text-white"
                          : "bg-muted text-muted-foreground"
                      }`}
                    >
                      {isDefault
                        ? t("kimi.form.defaultModel", { defaultValue: "默认模型" })
                        : t("kimi.form.fallbackModel", { defaultValue: "备选模型" })}
                    </span>
                    {isDefault && (
                      <span className="text-[10px] text-blue-600 dark:text-blue-400">
                        {t("kimi.form.defaultModelHint", {
                          defaultValue: "切换到此供应商时使用",
                        })}
                      </span>
                    )}
                  </div>

                <div className="flex items-center gap-2">
                  <div className="flex-1 space-y-1">
                    <label className="text-xs text-muted-foreground">
                      {t("kimi.form.modelId", { defaultValue: "模型别名" })}
                    </label>
                    <div className="flex gap-1">
                      <Input
                        value={model.id}
                        onChange={(e) =>
                          handleModelChange(index, "id", e.target.value)
                        }
                        placeholder={t("kimi.form.modelIdPlaceholder", {
                          defaultValue: "kimi-k2.7-code",
                        })}
                        className="flex-1"
                      />
                      {fetchedModels.length > 0 && (
                        <ModelDropdown
                          models={fetchedModels}
                          onSelect={(selected) => {
                            handleModelChange(index, "id", selected);
                            // 从 API 选择模型时同步填充请求时发送的模型 ID
                            if (!model.model || model.model === model.id) {
                              handleModelChange(index, "model", selected);
                            }
                          }}
                        />
                      )}
                    </div>
                  </div>
                  <div className="flex-1 space-y-1">
                    <label className="text-xs text-muted-foreground">
                      {t("kimi.form.wireModel", {
                        defaultValue: "模型 ID（请求时发送）",
                      })}
                    </label>
                    <Input
                      value={model.model ?? ""}
                      onChange={(e) =>
                        handleModelChange(index, "model", e.target.value)
                      }
                      placeholder={t("kimi.form.wireModelPlaceholder", {
                        defaultValue: "kimi-k2.7-code",
                      })}
                    />
                  </div>
                  <div className="flex-1 space-y-1">
                    <label className="text-xs text-muted-foreground">
                      {t("kimi.form.modelName", { defaultValue: "显示名称" })}
                    </label>
                    <Input
                      value={model.name ?? ""}
                      onChange={(e) =>
                        handleModelChange(index, "name", e.target.value)
                      }
                      placeholder={t("kimi.form.modelNamePlaceholder", {
                        defaultValue: "Kimi K2.7 Code",
                      })}
                    />
                  </div>
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon"
                    onClick={() => handleRemoveModel(index)}
                    className="h-9 w-9 mt-5 text-muted-foreground hover:text-destructive"
                  >
                    <Trash2 className="h-4 w-4" />
                  </Button>
                </div>

                <AdvancedSection
                  open={expandedModels[index] ?? false}
                  onOpenChange={() => toggleModelAdvanced(index)}
                  labelKey="kimi.form.advancedOptions"
                >
                  <div className="space-y-1">
                    <label className="text-xs text-muted-foreground">
                      {t("kimi.form.contextLength", {
                        defaultValue: "上下文长度（token）",
                      })}
                    </label>
                    <div className="flex gap-1.5">
                      <Select
                        value={
                          isPresetContextSize(model.max_context_size)
                            ? String(model.max_context_size)
                            : CUSTOM_OPTION
                        }
                        onValueChange={(v) => {
                          if (v === CUSTOM_OPTION) {
                            // 保留当前值，切到自定义输入框
                            handleModelChange(
                              index,
                              "max_context_size",
                              model.max_context_size,
                            );
                          } else {
                            handleModelChange(
                              index,
                              "max_context_size",
                              parseInt(v),
                            );
                          }
                        }}
                      >
                        <SelectTrigger className="flex-1">
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          {CONTEXT_SIZE_PRESETS.map((p) => (
                            <SelectItem
                              key={p.value}
                              value={String(p.value)}
                            >
                              {p.label}
                            </SelectItem>
                          ))}
                          <SelectItem value={CUSTOM_OPTION}>
                            {t("kimi.form.custom", { defaultValue: "自定义…" })}
                          </SelectItem>
                        </SelectContent>
                      </Select>
                      {!isPresetContextSize(model.max_context_size) && (
                        <Input
                          type="number"
                          className="w-36"
                          value={model.max_context_size ?? ""}
                          onChange={(e) =>
                            handleModelChange(
                              index,
                              "max_context_size",
                              e.target.value
                                ? parseInt(e.target.value)
                                : undefined,
                            )
                          }
                          placeholder="262144"
                        />
                      )}
                    </div>
                  </div>
                  <div className="grid grid-cols-2 gap-3">
                    <div className="space-y-1">
                      <label className="text-xs text-muted-foreground">
                        {t("kimi.form.maxInputSize", {
                          defaultValue: "输入上限（token）",
                        })}
                      </label>
                      <Input
                        type="number"
                        value={model.max_input_size ?? ""}
                        onChange={(e) =>
                          handleModelChange(
                            index,
                            "max_input_size",
                            e.target.value
                              ? parseInt(e.target.value)
                              : undefined,
                          )
                        }
                        placeholder="272000"
                      />
                    </div>
                    <div className="space-y-1">
                      <label className="text-xs text-muted-foreground">
                        {t("kimi.form.maxOutputSize", {
                          defaultValue: "输出上限（token）",
                        })}
                      </label>
                      <Input
                        type="number"
                        value={model.max_output_size ?? ""}
                        onChange={(e) =>
                          handleModelChange(
                            index,
                            "max_output_size",
                            e.target.value
                              ? parseInt(e.target.value)
                              : undefined,
                          )
                        }
                        placeholder="32768"
                      />
                    </div>
                  </div>
                  <div className="space-y-1">
                    <label className="text-xs text-muted-foreground">
                      {t("kimi.form.supportEfforts", {
                        defaultValue: "支持的推理强度（逗号分隔）",
                      })}
                    </label>
                    <Input
                      value={model.support_efforts?.join(", ") ?? ""}
                      onChange={(e) =>
                        handleModelChange(
                          index,
                          "support_efforts",
                          e.target.value
                            .split(",")
                            .map((s) => s.trim())
                            .filter(Boolean),
                        )
                      }
                      placeholder="low, medium, high, max"
                    />
                  </div>
                  <div className="space-y-1">
                    <label className="text-xs text-muted-foreground">
                      {t("kimi.form.defaultEffort", {
                        defaultValue: "默认推理强度",
                      })}
                    </label>
                    <div className="flex gap-1.5">
                      <Select
                        value={
                          model.default_effort &&
                          EFFORT_PRESETS.includes(model.default_effort)
                            ? model.default_effort
                            : CUSTOM_OPTION
                        }
                        onValueChange={(v) => {
                          handleModelChange(
                            index,
                            "default_effort",
                            v === CUSTOM_OPTION ? "" : v,
                          );
                        }}
                      >
                        <SelectTrigger className="flex-1">
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          {EFFORT_PRESETS.map((e) => (
                            <SelectItem key={e} value={e}>
                              {e}
                            </SelectItem>
                          ))}
                          <SelectItem value={CUSTOM_OPTION}>
                            {t("kimi.form.custom", { defaultValue: "自定义…" })}
                          </SelectItem>
                        </SelectContent>
                      </Select>
                      {model.default_effort &&
                        !EFFORT_PRESETS.includes(model.default_effort) && (
                          <Input
                            className="w-36"
                            value={model.default_effort ?? ""}
                            onChange={(e) =>
                              handleModelChange(
                                index,
                                "default_effort",
                                e.target.value,
                              )
                            }
                            placeholder="high"
                          />
                        )}
                    </div>
                  </div>
                  <div className="space-y-1">
                    <label className="text-xs text-muted-foreground">
                      {t("kimi.form.modelBaseUrl", {
                        defaultValue: "模型级端点覆盖",
                      })}
                    </label>
                    <Input
                      value={model.base_url ?? ""}
                      onChange={(e) =>
                        handleModelChange(index, "base_url", e.target.value)
                      }
                      placeholder="https://gateway.example.com/v1"
                    />
                  </div>
                  <div className="space-y-1">
                    <label className="text-xs text-muted-foreground">
                      {t("kimi.form.capabilities", {
                        defaultValue: "能力标签（逗号分隔）",
                      })}
                    </label>
                    <Input
                      value={model.capabilities?.join(", ") ?? ""}
                      onChange={(e) =>
                        handleModelChange(
                          index,
                          "capabilities",
                          e.target.value
                            .split(",")
                            .map((s) => s.trim())
                            .filter(Boolean),
                        )
                      }
                      placeholder="thinking, tool_use"
                    />
                  </div>
                </AdvancedSection>
              </div>
              );
            })}
          </div>
        )}

        <p className="text-xs text-muted-foreground">
          {t("kimi.form.modelsHint", {
            defaultValue:
              "切换到此供应商时，默认模型会写入顶层 default_model。",
          })}
        </p>
      </div>
    </>
  );
}
