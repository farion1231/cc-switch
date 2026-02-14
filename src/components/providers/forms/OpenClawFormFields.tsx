import { useTranslation } from "react-i18next";
import { useState, useRef, useCallback } from "react";
import { FormLabel } from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
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
import { Plus, Trash2, ChevronDown, ChevronRight } from "lucide-react";
import { ApiKeySection } from "./shared";
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
}: OpenClawFormFieldsProps) {
  const { t } = useTranslation();
  const [expandedModels, setExpandedModels] = useState<Record<number, boolean>>(
    {},
  );

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
  const toggleModelAdvanced = (index: number) => {
    setExpandedModels((prev) => ({ ...prev, [index]: !prev[index] }));
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
      },
    ]);
  };

  // Remove a model entry
  const handleRemoveModel = (index: number) => {
    modelKeysRef.current.splice(index, 1);
    const newModels = [...models];
    newModels.splice(index, 1);
    onModelsChange(newModels);
    // Clean up expanded state
    setExpandedModels((prev) => {
      const updated = { ...prev };
      delete updated[index];
      return updated;
    });
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
        <Input
          id="openclaw-baseurl"
          value={baseUrl}
          onChange={(e) => onBaseUrlChange(e.target.value)}
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
        category={category}
        shouldShowLink={shouldShowApiKeyLink}
        websiteUrl={websiteUrl}
        isPartner={isPartner}
        partnerPromotionKey={partnerPromotionKey}
      />

      {/* Models Editor */}
      <div className="space-y-3">
        <div className="flex items-center justify-between">
          <FormLabel>
            {t("openclaw.models", { defaultValue: "模型列表" })}
          </FormLabel>
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

        {models.length === 0 ? (
          <p className="text-sm text-muted-foreground py-2">
            {t("openclaw.noModels", {
              defaultValue: "暂无模型配置。点击添加模型来配置可用模型。",
            })}
          </p>
        ) : (
          <div className="space-y-4">
            {models.map((model, index) => (
              <div
                key={modelKeys[index]}
                className="p-3 border border-border/50 rounded-lg space-y-3"
              >
                {/* Model ID and Name row */}
                <div className="flex items-center gap-2">
                  <div className="flex-1 space-y-1">
                    <label className="text-xs text-muted-foreground">
                      {t("openclaw.modelId", { defaultValue: "模型 ID" })}
                    </label>
                    <Input
                      value={model.id}
                      onChange={(e) =>
                        handleModelChange(index, "id", e.target.value)
                      }
                      placeholder={t("openclaw.modelIdPlaceholder", {
                        defaultValue: "claude-3-sonnet",
                      })}
                    />
                  </div>
                  <div className="flex-1 space-y-1">
                    <label className="text-xs text-muted-foreground">
                      {t("openclaw.modelName", { defaultValue: "显示名称" })}
                    </label>
                    <Input
                      value={model.name}
                      onChange={(e) =>
                        handleModelChange(index, "name", e.target.value)
                      }
                      placeholder={t("openclaw.modelNamePlaceholder", {
                        defaultValue: "Claude 3 Sonnet",
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

                {/* Advanced Options (Collapsible) */}
                <Collapsible
                  open={expandedModels[index] ?? false}
                  onOpenChange={() => toggleModelAdvanced(index)}
                >
                  <CollapsibleTrigger asChild>
                    <Button
                      type="button"
                      variant="ghost"
                      size="sm"
                      className="h-7 gap-1 text-xs text-muted-foreground hover:text-foreground"
                    >
                      {expandedModels[index] ? (
                        <ChevronDown className="h-3.5 w-3.5" />
                      ) : (
                        <ChevronRight className="h-3.5 w-3.5" />
                      )}
                      {t("openclaw.advancedOptions", {
                        defaultValue: "高级选项",
                      })}
                    </Button>
                  </CollapsibleTrigger>
                  <CollapsibleContent className="space-y-3 pt-2">
                    {/* Context Window, Max Tokens and Reasoning row */}
                    <div className="flex items-center gap-2">
                      <div className="flex-1 space-y-1">
                        <label className="text-xs text-muted-foreground">
                          {t("openclaw.contextWindow", {
                            defaultValue: "上下文窗口",
                          })}
                        </label>
                        <Input
                          type="number"
                          value={model.contextWindow ?? ""}
                          onChange={(e) =>
                            handleModelChange(
                              index,
                              "contextWindow",
                              e.target.value
                                ? parseInt(e.target.value)
                                : undefined,
                            )
                          }
                          placeholder="200000"
                        />
                      </div>
                      <div className="flex-1 space-y-1">
                        <label className="text-xs text-muted-foreground">
                          {t("openclaw.maxTokens", {
                            defaultValue: "最大输出 Tokens",
                          })}
                        </label>
                        <Input
                          type="number"
                          value={model.maxTokens ?? ""}
                          onChange={(e) =>
                            handleModelChange(
                              index,
                              "maxTokens",
                              e.target.value
                                ? parseInt(e.target.value)
                                : undefined,
                            )
                          }
                          placeholder="32000"
                        />
                      </div>
                      <div className="flex-1 space-y-1">
                        <label className="text-xs text-muted-foreground">
                          {t("openclaw.reasoning", {
                            defaultValue: "推理模式",
                          })}
                        </label>
                        <div className="flex items-center h-9 gap-2">
                          <Switch
                            checked={model.reasoning ?? false}
                            onCheckedChange={(checked) =>
                              handleModelChange(index, "reasoning", checked)
                            }
                          />
                          <span className="text-xs text-muted-foreground">
                            {model.reasoning
                              ? t("openclaw.reasoningOn", {
                                  defaultValue: "启用",
                                })
                              : t("openclaw.reasoningOff", {
                                  defaultValue: "关闭",
                                })}
                          </span>
                        </div>
                      </div>
                    </div>

                    {/* Cost row */}
                    <div className="flex items-center gap-2">
                      <div className="flex-1 space-y-1">
                        <label className="text-xs text-muted-foreground">
                          {t("openclaw.inputCost", {
                            defaultValue: "输入价格 ($/M tokens)",
                          })}
                        </label>
                        <Input
                          type="number"
                          step="0.001"
                          value={model.cost?.input ?? ""}
                          onChange={(e) =>
                            handleCostChange(index, "input", e.target.value)
                          }
                          placeholder="3"
                        />
                      </div>
                      <div className="flex-1 space-y-1">
                        <label className="text-xs text-muted-foreground">
                          {t("openclaw.outputCost", {
                            defaultValue: "输出价格 ($/M tokens)",
                          })}
                        </label>
                        <Input
                          type="number"
                          step="0.001"
                          value={model.cost?.output ?? ""}
                          onChange={(e) =>
                            handleCostChange(index, "output", e.target.value)
                          }
                          placeholder="15"
                        />
                      </div>
                      <div className="flex-1" />
                    </div>

                    {/* Cache Cost row */}
                    <div className="flex items-center gap-2">
                      <div className="flex-1 space-y-1">
                        <label className="text-xs text-muted-foreground">
                          {t("openclaw.cacheReadCost", {
                            defaultValue: "缓存读取价格 ($/M tokens)",
                          })}
                        </label>
                        <Input
                          type="number"
                          step="0.001"
                          value={model.cost?.cacheRead ?? ""}
                          onChange={(e) =>
                            handleCostChange(index, "cacheRead", e.target.value)
                          }
                          placeholder="0.3"
                        />
                      </div>
                      <div className="flex-1 space-y-1">
                        <label className="text-xs text-muted-foreground">
                          {t("openclaw.cacheWriteCost", {
                            defaultValue: "缓存写入价格 ($/M tokens)",
                          })}
                        </label>
                        <Input
                          type="number"
                          step="0.001"
                          value={model.cost?.cacheWrite ?? ""}
                          onChange={(e) =>
                            handleCostChange(
                              index,
                              "cacheWrite",
                              e.target.value,
                            )
                          }
                          placeholder="3.75"
                        />
                      </div>
                      <div className="flex-1" />
                    </div>
                    <p className="text-xs text-muted-foreground">
                      {t("openclaw.cacheCostHint", {
                        defaultValue:
                          "缓存价格用于计算 Prompt Caching 的成本。如不使用缓存可留空。",
                      })}
                    </p>
                  </CollapsibleContent>
                </Collapsible>
              </div>
            ))}
          </div>
        )}

        <p className="text-xs text-muted-foreground">
          {t("openclaw.modelsHint", {
            defaultValue:
              "配置该供应商支持的模型。模型 ID 用于 API 调用，显示名称用于界面展示。",
          })}
        </p>
      </div>
    </>
  );
}
