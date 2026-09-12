import { useTranslation } from "react-i18next";
import { useState, useRef, useCallback } from "react";
import { FormLabel } from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import { ImeSafeInput } from "@/components/ui/ime-safe-input";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { toast } from "sonner";
import { Download, Plus, Trash2, ChevronRight, Loader2 } from "lucide-react";
import { Checkbox } from "@/components/ui/checkbox";
import { ApiKeySection, ModelDropdown } from "./shared";
import {
  fetchModelsForConfig,
  showFetchModelsError,
  type FetchedModel,
} from "@/lib/api/model-fetch";
import { openclawApiProtocols } from "@/config/openclawProviderPresets";
import type { ProviderCategory, OpenClawModel } from "@/types";

interface OpenClawFormFieldsProps {
  // Base URL
  baseUrl: string;
  onBaseUrlChange: (value: string) => void;

  // API Key
  apiKey: string;
  onApiKeyChange: (value: string) => void;
  category?: ProviderCategory;
  shouldShowApiKeyLink: boolean;
  websiteUrl: string;
  isPartner?: boolean;
  partnerPromotionKey?: string;

  // API Protocol
  api: string;
  onApiChange: (value: string) => void;

  // Models
  models: OpenClawModel[];
  onModelsChange: (models: OpenClawModel[]) => void;

  // User-Agent
  userAgent: boolean;
  onUserAgentChange: (checked: boolean) => void;
}

export function OpenClawFormFields({
  baseUrl,
  onBaseUrlChange,
  apiKey,
  onApiKeyChange,
  category,
  shouldShowApiKeyLink,
  websiteUrl,
  isPartner,
  partnerPromotionKey,
  api,
  onApiChange,
  models,
  onModelsChange,
  userAgent,
  onUserAgentChange,
}: OpenClawFormFieldsProps) {
  const { t } = useTranslation();
  const [expandedModels, setExpandedModels] = useState<Set<string>>(new Set());
  const [fetchedModels, setFetchedModels] = useState<FetchedModel[]>([]);
  const [isFetchingModels, setIsFetchingModels] = useState(false);

  // Stable key tracking for models list
  const modelKeysRef = useRef<string[]>([]);
  const getModelKeys = useCallback(() => {
    // Grow keys array if models were added externally
    while (modelKeysRef.current.length < models.length) {
      modelKeysRef.current.push(crypto.randomUUID());
    }
    // Shrink if models were removed externally
    if (modelKeysRef.current.length > models.length) {
      modelKeysRef.current.length = models.length;
    }
    return modelKeysRef.current;
  }, [models.length]);
  const modelKeys = getModelKeys();

  // Toggle advanced section for a model
  const toggleModelAdvanced = (modelKey: string) => {
    setExpandedModels((prev) => {
      const next = new Set(prev);
      if (next.has(modelKey)) {
        next.delete(modelKey);
      } else {
        next.add(modelKey);
      }
      return next;
    });
  };

  // Add a new model entry
  const handleAddModel = () => {
    modelKeysRef.current.push(crypto.randomUUID());
    onModelsChange([
      ...models,
      {
        id: "",
        name: "",
        contextWindow: undefined,
        maxTokens: undefined,
        cost: undefined,
        input: ["text"],
      },
    ]);
  };

  // Fetch models from API
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
      .then((models) => {
        setFetchedModels(models);
        if (models.length === 0) {
          toast.info(t("providerForm.fetchModelsEmpty"));
        } else {
          toast.success(
            t("providerForm.fetchModelsSuccess", { count: models.length }),
          );
        }
      })
      .catch((err) => {
        console.warn("[ModelFetch] Failed:", err);
        showFetchModelsError(err, t);
      })
      .finally(() => setIsFetchingModels(false));
  }, [baseUrl, apiKey, t]);

  // Remove a model entry
  const handleRemoveModel = (index: number) => {
    const removedKey = modelKeysRef.current[index];
    modelKeysRef.current.splice(index, 1);
    const newModels = [...models];
    newModels.splice(index, 1);
    onModelsChange(newModels);
    if (removedKey) {
      setExpandedModels((prev) => {
        const next = new Set(prev);
        next.delete(removedKey);
        return next;
      });
    }
  };

  // Update model field
  const handleModelChange = (
    index: number,
    field: keyof OpenClawModel,
    value: unknown,
  ) => {
    const newModels = [...models];
    newModels[index] = { ...newModels[index], [field]: value };
    onModelsChange(newModels);
  };

  // Update model cost
  const handleCostChange = (
    index: number,
    costField: "input" | "output" | "cacheRead" | "cacheWrite",
    value: string,
  ) => {
    const newModels = [...models];
    const numValue = parseFloat(value);
    const currentCost = newModels[index].cost || { input: 0, output: 0 };
    newModels[index] = {
      ...newModels[index],
      cost: {
        ...currentCost,
        [costField]: isNaN(numValue) ? undefined : numValue,
      },
    };
    onModelsChange(newModels);
  };

  return (
    <>
      {/* API Protocol Selector */}
      <div className="space-y-2">
        <FormLabel htmlFor="openclaw-api">
          {t("openclaw.apiProtocol", {
            defaultValue: "API 协议",
          })}
        </FormLabel>
        <Select value={api} onValueChange={onApiChange}>
          <SelectTrigger id="openclaw-api">
            <SelectValue
              placeholder={t("openclaw.selectProtocol", {
                defaultValue: "选择 API 协议",
              })}
            />
          </SelectTrigger>
          <SelectContent>
            {openclawApiProtocols.map((protocol) => (
              <SelectItem key={protocol.value} value={protocol.value}>
                {protocol.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <p className="text-xs text-muted-foreground">
          {t("openclaw.apiProtocolHint", {
            defaultValue:
              "选择与供应商 API 兼容的协议类型。大多数供应商使用 OpenAI Completions 格式。",
          })}
        </p>
      </div>

      {/* Base URL */}
      <div className="space-y-2">
        <FormLabel htmlFor="openclaw-baseurl">
          {t("openclaw.baseUrl", { defaultValue: "API 端点" })}
        </FormLabel>
        <ImeSafeInput
          id="openclaw-baseurl"
          value={baseUrl}
          onValueChange={onBaseUrlChange}
          placeholder="https://api.example.com/v1"
        />
        <p className="text-xs text-muted-foreground">
          {t("openclaw.baseUrlHint", {
            defaultValue: "供应商的 API 端点地址。",
          })}
        </p>
      </div>

      {/* API Key */}
      <ApiKeySection
        value={apiKey}
        onChange={onApiKeyChange}
        // OpenClaw 的 API key 始终由用户自填，没有 OAuth-only 的免 key 官方供应商，
        // 故不让 official 禁用输入框（与 Hermes 对齐）。
        category={category === "official" ? undefined : category}
        shouldShowLink={shouldShowApiKeyLink}
        websiteUrl={websiteUrl}
        isPartner={isPartner}
        partnerPromotionKey={partnerPromotionKey}
      />

      {/* User-Agent */}
      <div className="flex items-center justify-between border-l border-border-default pl-3">
        <div className="space-y-0.5">
          <FormLabel>
            {t("openclaw.userAgent", { defaultValue: "发送 User-Agent" })}
          </FormLabel>
          <p className="text-xs text-muted-foreground">
            {t("openclaw.userAgentHint", {
              defaultValue: "部分供应商需要浏览器 User-Agent 才能正常访问。",
            })}
          </p>
        </div>
        <Switch checked={userAgent} onCheckedChange={onUserAgentChange} />
      </div>

      {/* Models Editor */}
      <div className="space-y-3 border-l border-border-default pl-3">
        <div className="flex items-center justify-between gap-3">
          <FormLabel>
            {t("openclaw.models", { defaultValue: "模型配置" })}
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
              type="button"
              variant="outline"
              size="sm"
              onClick={handleAddModel}
              className="h-7 gap-1"
            >
              <Plus className="h-3.5 w-3.5" />
              {t("openclaw.addModel", { defaultValue: "添加模型" })}
            </Button>
          </div>
        </div>

        {models.length === 0 ? (
          <p role="status" className="py-2 text-sm text-muted-foreground">
            {t("openclaw.noModels", {
              defaultValue: "暂无模型配置",
            })}
          </p>
        ) : (
          <div className="space-y-2">
            <div className="flex items-center gap-2 px-1 text-xs text-muted-foreground">
              <span className="w-9" />
              <span className="flex-1">
                {t("openclaw.modelId", { defaultValue: "模型 ID" })}
              </span>
              <span className="flex-1">
                {t("openclaw.modelName", { defaultValue: "显示名称" })}
              </span>
              <span className="w-9" />
            </div>
            {models.map((model, index) => {
              const modelKey = modelKeys[index];
              const isExpanded = expandedModels.has(modelKey);
              return (
                <div key={modelKey} className="space-y-2">
                  <div className="flex items-center gap-2">
                    <Button
                      type="button"
                      variant="ghost"
                      size="icon"
                      onClick={() => toggleModelAdvanced(modelKey)}
                      aria-expanded={isExpanded}
                      aria-label={t("openclaw.toggleModelDetails", {
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
                      <ImeSafeInput
                        value={model.id}
                        onValueChange={(value) =>
                          handleModelChange(index, "id", value)
                        }
                        placeholder={t("openclaw.modelIdPlaceholder", {
                          defaultValue: "claude-3-sonnet",
                        })}
                        aria-label={t("openclaw.modelId", {
                          defaultValue: "模型 ID",
                        })}
                        className="min-w-0 flex-1"
                      />
                      {fetchedModels.length > 0 && (
                        <ModelDropdown
                          models={fetchedModels}
                          onSelect={(id) => handleModelChange(index, "id", id)}
                        />
                      )}
                    </div>
                    <ImeSafeInput
                      value={model.name}
                      onValueChange={(value) =>
                        handleModelChange(index, "name", value)
                      }
                      placeholder={t("openclaw.modelNamePlaceholder", {
                        defaultValue: "Claude 3 Sonnet",
                      })}
                      aria-label={t("openclaw.modelName", {
                        defaultValue: "显示名称",
                      })}
                      className="min-w-0 flex-1"
                    />
                    <Button
                      type="button"
                      variant="ghost"
                      size="icon"
                      onClick={() => handleRemoveModel(index)}
                      aria-label={t("openclaw.removeModel", {
                        defaultValue: "移除模型",
                      })}
                      className="h-9 w-9 shrink-0 text-muted-foreground hover:text-destructive"
                    >
                      <Trash2 className="h-4 w-4" />
                    </Button>
                  </div>

                  {isExpanded && (
                    <div className="ml-9 grid gap-3 border-l-2 border-muted pl-4 sm:grid-cols-2 animate-in fade-in-0 slide-in-from-top-1 duration-200 motion-reduce:animate-none">
                      <div className="flex min-h-9 flex-wrap items-center gap-x-8 gap-y-2 sm:col-span-2">
                        <div className="flex items-center gap-2.5">
                          <FormLabel
                            htmlFor={`openclaw-model-reasoning-${modelKey}`}
                            className="cursor-pointer"
                          >
                            {t("openclaw.reasoning", {
                              defaultValue: "支持扩展思考",
                            })}
                          </FormLabel>
                          <Switch
                            id={`openclaw-model-reasoning-${modelKey}`}
                            checked={model.reasoning ?? false}
                            onCheckedChange={(checked) =>
                              handleModelChange(index, "reasoning", checked)
                            }
                          />
                        </div>
                        <div className="flex items-center gap-3">
                          <FormLabel>
                            {t("openclaw.inputTypes", {
                              defaultValue: "输入类型",
                            })}
                          </FormLabel>
                          {(["text", "image"] as const).map((type) => (
                            <label
                              key={type}
                              className="flex cursor-pointer select-none items-center gap-1.5"
                            >
                              <Checkbox
                                checked={(model.input ?? ["text"]).includes(
                                  type,
                                )}
                                onCheckedChange={(checked) => {
                                  const current = model.input ?? ["text"];
                                  const next = checked
                                    ? [...new Set([...current, type])]
                                    : current.filter((value) => value !== type);
                                  handleModelChange(index, "input", next);
                                }}
                              />
                              <span className="text-xs">{type}</span>
                            </label>
                          ))}
                        </div>
                      </div>

                      <div className="space-y-1">
                        <FormLabel
                          htmlFor={`openclaw-model-context-${modelKey}`}
                          className="text-xs text-muted-foreground"
                        >
                          {t("openclaw.contextWindow", {
                            defaultValue: "上下文长度",
                          })}
                        </FormLabel>
                        <Input
                          id={`openclaw-model-context-${modelKey}`}
                          type="number"
                          min={1}
                          step={1}
                          value={model.contextWindow ?? ""}
                          onChange={(event) =>
                            handleModelChange(
                              index,
                              "contextWindow",
                              event.target.value
                                ? parseInt(event.target.value)
                                : undefined,
                            )
                          }
                          placeholder="200000"
                        />
                      </div>
                      <div className="space-y-1">
                        <FormLabel
                          htmlFor={`openclaw-model-max-tokens-${modelKey}`}
                          className="text-xs text-muted-foreground"
                        >
                          {t("openclaw.maxTokens", {
                            defaultValue: "最大输出 Token 数",
                          })}
                        </FormLabel>
                        <Input
                          id={`openclaw-model-max-tokens-${modelKey}`}
                          type="number"
                          min={1}
                          step={1}
                          value={model.maxTokens ?? ""}
                          onChange={(event) =>
                            handleModelChange(
                              index,
                              "maxTokens",
                              event.target.value
                                ? parseInt(event.target.value)
                                : undefined,
                            )
                          }
                          placeholder="32000"
                        />
                      </div>

                      <div className="space-y-2 sm:col-span-2">
                        <span className="text-xs font-medium text-muted-foreground">
                          {t("openclaw.modelCost", {
                            defaultValue: "成本（$/百万 Token）",
                          })}
                        </span>
                        <div className="grid gap-2 sm:grid-cols-2">
                          <div className="space-y-1">
                            <FormLabel
                              htmlFor={`openclaw-model-input-cost-${modelKey}`}
                              className="text-xs text-muted-foreground"
                            >
                              {t("openclaw.inputCost", {
                                defaultValue: "输入",
                              })}
                            </FormLabel>
                            <Input
                              id={`openclaw-model-input-cost-${modelKey}`}
                              type="number"
                              min={0}
                              step="0.001"
                              value={model.cost?.input ?? ""}
                              onChange={(event) =>
                                handleCostChange(
                                  index,
                                  "input",
                                  event.target.value,
                                )
                              }
                              placeholder="3"
                            />
                          </div>
                          <div className="space-y-1">
                            <FormLabel
                              htmlFor={`openclaw-model-output-cost-${modelKey}`}
                              className="text-xs text-muted-foreground"
                            >
                              {t("openclaw.outputCost", {
                                defaultValue: "输出",
                              })}
                            </FormLabel>
                            <Input
                              id={`openclaw-model-output-cost-${modelKey}`}
                              type="number"
                              min={0}
                              step="0.001"
                              value={model.cost?.output ?? ""}
                              onChange={(event) =>
                                handleCostChange(
                                  index,
                                  "output",
                                  event.target.value,
                                )
                              }
                              placeholder="15"
                            />
                          </div>
                          <div className="space-y-1">
                            <FormLabel
                              htmlFor={`openclaw-model-cache-read-${modelKey}`}
                              className="text-xs text-muted-foreground"
                            >
                              {t("openclaw.cacheReadCost", {
                                defaultValue: "缓存读取",
                              })}
                            </FormLabel>
                            <Input
                              id={`openclaw-model-cache-read-${modelKey}`}
                              type="number"
                              min={0}
                              step="0.001"
                              value={model.cost?.cacheRead ?? ""}
                              onChange={(event) =>
                                handleCostChange(
                                  index,
                                  "cacheRead",
                                  event.target.value,
                                )
                              }
                              placeholder="0.3"
                            />
                          </div>
                          <div className="space-y-1">
                            <FormLabel
                              htmlFor={`openclaw-model-cache-write-${modelKey}`}
                              className="text-xs text-muted-foreground"
                            >
                              {t("openclaw.cacheWriteCost", {
                                defaultValue: "缓存写入",
                              })}
                            </FormLabel>
                            <Input
                              id={`openclaw-model-cache-write-${modelKey}`}
                              type="number"
                              min={0}
                              step="0.001"
                              value={model.cost?.cacheWrite ?? ""}
                              onChange={(event) =>
                                handleCostChange(
                                  index,
                                  "cacheWrite",
                                  event.target.value,
                                )
                              }
                              placeholder="3.75"
                            />
                          </div>
                        </div>
                      </div>
                    </div>
                  )}
                </div>
              );
            })}
          </div>
        )}
      </div>
    </>
  );
}
