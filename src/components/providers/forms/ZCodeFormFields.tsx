import { useState, useEffect, useCallback, useRef } from "react";
import { useTranslation } from "react-i18next";
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
import { Plus, Trash2, ChevronRight } from "lucide-react";
import { toast } from "@/lib/toast";
import {
  fetchModelsForConfig,
  showFetchModelsError,
  type FetchedModel,
} from "@/lib/api/model-fetch";
import {
  FetchedModelPicker,
  type FetchedModelSelection,
} from "./FetchedModelPicker";
import { ApiKeySection, EndpointField } from "./shared";
import EndpointSpeedTest from "./EndpointSpeedTest";
import { zcodeProviderKinds } from "@/config/zcodeProviderPresets";
import { cn } from "@/lib/utils";
import { useModelMetadataFill } from "@/hooks/useModelMetadataFill";
import { fillZCodeModel, metadataFilledAnything } from "./modelMetadataFill";
import type { EndpointCandidate, ProviderCategory, ZCodeModel } from "@/types";

/**
 * Model ID input with local state to prevent focus loss on key change.
 * Mirrors the OpenCode ModelIdInput pattern.
 */
function ModelIdInput({
  modelId,
  onChange,
  placeholder,
}: {
  modelId: string;
  onChange: (newId: string) => void;
  placeholder?: string;
}) {
  const [localValue, setLocalValue] = useState(modelId);

  useEffect(() => {
    setLocalValue(modelId);
  }, [modelId]);

  return (
    <Input
      value={localValue}
      onChange={(e) => setLocalValue(e.target.value)}
      onBlur={() => {
        if (localValue !== modelId && localValue.trim()) {
          onChange(localValue);
        }
      }}
      placeholder={placeholder}
      className="flex-1"
    />
  );
}

interface ZCodeFormFieldsProps {
  // Provider Kind
  kind: string;
  onKindChange: (value: string) => void;

  // Provider display name (config.json provider.<id>.name)
  name: string;
  onNameChange: (value: string) => void;

  // API Key
  apiKey: string;
  onApiKeyChange: (value: string) => void;
  category?: ProviderCategory;
  shouldShowApiKeyLink: boolean;
  websiteUrl: string;
  isPartner?: boolean;
  partnerPromotionKey?: string;

  // Base URL
  baseUrl: string;
  onBaseUrlChange: (value: string) => void;

  // 端点测速
  providerId?: string;
  speedTestEndpoints: EndpointCandidate[];
  isEndpointModalOpen: boolean;
  onEndpointModalToggle: (open: boolean) => void;
  onCustomEndpointsChange?: (urls: string[]) => void;
  autoSelect: boolean;
  onAutoSelectChange: (checked: boolean) => void;

  // Models
  models: Record<string, ZCodeModel>;
  onModelsChange: (models: Record<string, ZCodeModel>) => void;
}

export function ZCodeFormFields({
  kind,
  onKindChange,
  name,
  onNameChange,
  apiKey,
  onApiKeyChange,
  category,
  shouldShowApiKeyLink,
  websiteUrl,
  isPartner,
  partnerPromotionKey,
  baseUrl,
  onBaseUrlChange,
  providerId,
  speedTestEndpoints,
  isEndpointModalOpen,
  onEndpointModalToggle,
  onCustomEndpointsChange,
  autoSelect,
  onAutoSelectChange,
  models,
  onModelsChange,
}: ZCodeFormFieldsProps) {
  const { t } = useTranslation();

  // Track which models have expanded details panel
  const [expandedModels, setExpandedModels] = useState<Set<string>>(new Set());

  // models 由父组件持有；fill 的异步回调用 ref 读最新一行，确认没被用户改掉。
  const modelsRef = useRef(models);
  modelsRef.current = models;

  const [fetchedModels, setFetchedModels] = useState<FetchedModel[]>([]);
  const [isFetchingModels, setIsFetchingModels] = useState(false);
  const modelFetchGeneration = useRef(0);

  // 地址、Key、协议变了，之前拉到的列表就不作数了
  useEffect(() => {
    setFetchedModels((prev) => (prev.length === 0 ? prev : []));
    setIsFetchingModels(false);
    return () => {
      modelFetchGeneration.current += 1;
    };
  }, [baseUrl, apiKey, kind]);

  const handleFetchModels = useCallback(() => {
    if (!baseUrl || !apiKey) {
      showFetchModelsError(null, t, {
        hasApiKey: Boolean(apiKey),
        hasBaseUrl: Boolean(baseUrl),
      });
      return;
    }
    const generation = ++modelFetchGeneration.current;
    setFetchedModels([]);
    setIsFetchingModels(true);
    // Anthropic 协议的模型列表要用 x-api-key 认证，其余按默认的 Bearer
    fetchModelsForConfig(baseUrl, apiKey, undefined, undefined, undefined, {
      apiFormat: kind === "anthropic" ? "anthropic-messages" : undefined,
    })
      .then((result) => {
        if (generation !== modelFetchGeneration.current) return;
        const unique = [
          ...new Map(result.map((model) => [model.id, model])).values(),
        ];
        setFetchedModels(unique);
        if (unique.length === 0) {
          toast.info(t("providerForm.fetchModelsEmpty"));
        }
      })
      .catch((err) => {
        if (generation !== modelFetchGeneration.current) return;
        console.warn("[ModelFetch] Failed:", err);
        showFetchModelsError(err, t);
      })
      .finally(() => {
        if (generation === modelFetchGeneration.current) {
          setIsFetchingModels(false);
        }
      });
  }, [baseUrl, apiKey, kind, t]);

  // 预设不带窗口参数：只从 models.dev 补。手填的在 Model ID 提交（blur）那一刻补，
  // 从列表选的在加入时补。
  const fillModelMetadata = useModelMetadataFill({
    baseUrl,
    presets: () => [],
    prefetch: fetchedModels.length > 0,
  });
  const fillModelMetadataFor = (key: string, id: string, silent?: boolean) =>
    fillModelMetadata(
      id,
      (metadata) => {
        const model = modelsRef.current[key];
        if (model === undefined) return false;
        const filled = fillZCodeModel(model, metadata);
        if (!metadataFilledAnything(model, filled)) return false;
        onModelsChange({ ...modelsRef.current, [key]: filled });
        return true;
      },
      { silent },
    );

  const handleApplyFetchedModels = ({ add, remove }: FetchedModelSelection) => {
    const next = { ...models };
    for (const id of remove) delete next[id];
    const additions = add.filter(
      (id) => !Object.prototype.hasOwnProperty.call(next, id),
    );
    for (const id of additions) next[id] = {};
    onModelsChange(next);
    // fill 的「当场」一拨在父组件重渲染前运行，先把 ref 对齐到刚提交的行。
    modelsRef.current = next;
    if (remove.length > 0) {
      setExpandedModels((prev) => {
        const kept = new Set(prev);
        for (const id of remove) kept.delete(id);
        return kept;
      });
    }
    // 弹窗里已经显示过这些参数，补完不再提示。
    for (const id of additions) fillModelMetadataFor(id, id, true);
  };

  const toggleModelExpand = (key: string) => {
    setExpandedModels((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  };

  const handleAddModel = () => {
    const newKey = `model-${Date.now()}`;
    onModelsChange({
      ...models,
      [newKey]: {},
    });
  };

  const handleRemoveModel = (key: string) => {
    const newModels = { ...models };
    delete newModels[key];
    onModelsChange(newModels);
    setExpandedModels((prev) => {
      const next = new Set(prev);
      next.delete(key);
      return next;
    });
  };

  const handleModelIdChange = (oldKey: string, newKey: string) => {
    if (oldKey === newKey || !newKey.trim()) return;
    const newModels: Record<string, ZCodeModel> = {};
    for (const [k, v] of Object.entries(models)) {
      if (k === oldKey) {
        // 显示名不再可编辑也不写入 ZCode，改了 ID 就别把旧模型的名字带过去
        const renamed = { ...v };
        delete renamed.name;
        newModels[newKey] = renamed;
      } else {
        newModels[k] = v;
      }
    }
    onModelsChange(newModels);
    // fill 的「当场」一拨在父组件重渲染前运行，先把 ref 对齐到刚提交的行。
    modelsRef.current = newModels;
    fillModelMetadataFor(newKey, newKey);
    if (expandedModels.has(oldKey)) {
      setExpandedModels((prev) => {
        const next = new Set(prev);
        next.delete(oldKey);
        next.add(newKey);
        return next;
      });
    }
  };

  const handleModelLimitChange = (
    modelKey: string,
    limitKey: "context" | "output",
    value: string,
  ) => {
    const model = models[modelKey];
    const nextLimit = { ...(model.limit || {}) };
    const trimmedValue = value.trim();

    if (trimmedValue === "") {
      delete nextLimit[limitKey];
    } else {
      const parsed = Number(trimmedValue);
      // ZCode 只收正整数，0 写进去会让整份个人配置作废
      if (!Number.isFinite(parsed) || parsed < 1) return;
      nextLimit[limitKey] = Math.trunc(parsed);
    }

    const nextModel = { ...model };
    if (Object.keys(nextLimit).length > 0) {
      nextModel.limit = nextLimit;
    } else {
      delete nextModel.limit;
    }

    onModelsChange({
      ...models,
      [modelKey]: nextModel,
    });
  };

  return (
    <>
      {/* ZCode 是 additive 模式：写入的供应商不会自动生效，必须在 ZCode 内手动选择 */}
      <div className="rounded-md border border-border-default bg-muted/40 px-3 py-2.5 text-xs text-muted-foreground">
        {t("zcode.activationHint", {
          defaultValue:
            "ZCode keeps all providers live at once. After saving, switch to this provider inside ZCode's provider picker to actually use it.",
        })}
      </div>

      {/* Provider Kind Selector */}
      <div className="space-y-2">
        <FormLabel htmlFor="zcode-kind">
          {t("zcode.kind", { defaultValue: "Provider Kind" })}
        </FormLabel>
        <Select value={kind} onValueChange={onKindChange}>
          <SelectTrigger id="zcode-kind">
            <SelectValue
              placeholder={t("zcode.kindPlaceholder", {
                defaultValue: "Select a provider kind",
              })}
            />
          </SelectTrigger>
          <SelectContent>
            {zcodeProviderKinds.map((k) => (
              <SelectItem key={k.value} value={k.value}>
                {k.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <p className="text-xs text-muted-foreground">
          {t("zcode.kindHint", {
            defaultValue:
              "Select the API protocol this provider speaks (Anthropic, OpenAI, or OpenAI-Compatible).",
          })}
        </p>
      </div>

      {/* Provider display name (optional) */}
      <div className="space-y-2">
        <FormLabel htmlFor="zcode-name">
          {t("zcode.providerName", { defaultValue: "Provider Name" })}
        </FormLabel>
        <Input
          id="zcode-name"
          value={name}
          onChange={(e) => onNameChange(e.target.value)}
          placeholder={t("zcode.providerNamePlaceholder", {
            defaultValue: "My Provider",
          })}
        />
        <p className="text-xs text-muted-foreground">
          {t("zcode.providerNameHint", {
            defaultValue:
              "Optional display name stored in config.json. Defaults to the provider key if left empty.",
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

      {/* Base URL */}
      <div className="space-y-2">
        <EndpointField
          id="zcode-baseurl"
          label={t("zcode.baseUrl", { defaultValue: "Base URL" })}
          value={baseUrl}
          onChange={onBaseUrlChange}
          placeholder="https://api.example.com/v1"
          help={t("zcode.baseUrlHint", {
            defaultValue: "The base URL for the API endpoint.",
          })}
          onManageClick={() => onEndpointModalToggle(true)}
        />
      </div>

      {/* 端点测速弹窗 */}
      {isEndpointModalOpen && (
        <EndpointSpeedTest
          appId="zcode"
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

      {/* Models Editor */}
      <div className="space-y-3">
        <div className="flex items-center justify-between">
          <FormLabel>{t("zcode.models", { defaultValue: "Models" })}</FormLabel>
          <div className="flex gap-1">
            <FetchedModelPicker
              models={fetchedModels}
              configuredModelIds={Object.keys(models)}
              onApply={handleApplyFetchedModels}
              onFetch={handleFetchModels}
              isFetching={isFetchingModels}
              metadataSource={{ baseUrl }}
            />
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={handleAddModel}
              className="h-7 gap-1"
            >
              <Plus className="h-3.5 w-3.5" />
              {t("zcode.addModel", { defaultValue: "Add Model" })}
            </Button>
          </div>
        </div>

        {Object.keys(models).length === 0 ? (
          <p className="text-sm text-muted-foreground py-2">
            {t("zcode.noModels", { defaultValue: "No models configured" })}
          </p>
        ) : (
          <div className="space-y-2">
            {Object.entries(models).map(([key, model]) => (
              <div key={key} className="space-y-2">
                <div className="flex items-center gap-2">
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon"
                    onClick={() => toggleModelExpand(key)}
                    aria-label={t("zcode.toggleModelDetails", {
                      defaultValue: "Toggle model details",
                    })}
                    className="h-9 w-9 shrink-0"
                  >
                    <ChevronRight
                      className={cn(
                        "h-4 w-4 transition-transform",
                        expandedModels.has(key) && "rotate-90",
                      )}
                    />
                  </Button>
                  <div className="flex-1">
                    <ModelIdInput
                      modelId={key}
                      onChange={(newId) => handleModelIdChange(key, newId)}
                      placeholder={t("zcode.modelId", {
                        defaultValue: "Model ID",
                      })}
                    />
                  </div>
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon"
                    onClick={() => handleRemoveModel(key)}
                    className="h-9 w-9 text-muted-foreground hover:text-destructive"
                  >
                    <Trash2 className="h-4 w-4" />
                  </Button>
                </div>

                {expandedModels.has(key) && (
                  <div className="ml-9 pl-4 border-l-2 border-muted space-y-3">
                    <div className="space-y-2">
                      <span className="text-xs font-medium text-muted-foreground">
                        {t("zcode.modelLimits", {
                          defaultValue: "Token Limits",
                        })}
                      </span>
                      <div className="grid grid-cols-1 sm:grid-cols-2 gap-2">
                        <div className="space-y-1">
                          <FormLabel
                            htmlFor={`zcode-${key}-limit-context`}
                            className="text-xs text-muted-foreground"
                          >
                            {t("zcode.limitContext", {
                              defaultValue: "Context",
                            })}
                          </FormLabel>
                          <Input
                            id={`zcode-${key}-limit-context`}
                            type="number"
                            min={1}
                            step={1}
                            value={model.limit?.context ?? ""}
                            onChange={(e) =>
                              handleModelLimitChange(
                                key,
                                "context",
                                e.target.value,
                              )
                            }
                          />
                        </div>
                        <div className="space-y-1">
                          <FormLabel
                            htmlFor={`zcode-${key}-limit-output`}
                            className="text-xs text-muted-foreground"
                          >
                            {t("zcode.limitOutput", {
                              defaultValue: "Output",
                            })}
                          </FormLabel>
                          <Input
                            id={`zcode-${key}-limit-output`}
                            type="number"
                            min={1}
                            step={1}
                            value={model.limit?.output ?? ""}
                            onChange={(e) =>
                              handleModelLimitChange(
                                key,
                                "output",
                                e.target.value,
                              )
                            }
                          />
                        </div>
                      </div>
                      <p className="text-xs text-muted-foreground">
                        {t("zcode.modelLimitsHint", {
                          defaultValue:
                            "Leave blank to use ZCode's recommended values.",
                        })}
                      </p>
                    </div>
                  </div>
                )}
              </div>
            ))}
          </div>
        )}

        <p className="text-xs text-muted-foreground">
          {t("zcode.modelsHint", {
            defaultValue: "Model ID is the model name sent in API requests.",
          })}
        </p>
      </div>
    </>
  );
}
