import { useTranslation } from "react-i18next";
import { FormLabel } from "@/components/ui/form";
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectLabel,
  SelectSeparator,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Input } from "@/components/ui/input";
import type { ProviderCategory } from "@/types";
import {
  getQoderCliModelDisplayLabel,
  getQoderCliPlanLabel,
  getQoderCliPreset,
  getQoderCliSupportedPlanTypes,
  isQoderCliSupportedModel,
  type QoderCliPlanType,
  type QoderCliPresetModel,
} from "@/config/qodercliProviderPresets";
import { ApiKeySection } from "./shared";

const CUSTOM_MODEL_VALUE = "__qodercli_custom_model__";

interface QoderCliFormFieldsProps {
  providerKey: string;
  apiKey: string;
  onApiKeyChange: (value: string) => void;
  category?: ProviderCategory;
  shouldShowApiKeyLink: boolean;
  websiteUrl: string;
  isPartner?: boolean;
  partnerPromotionKey?: string;
  models: QoderCliPresetModel[];
  onModelsChange: (models: QoderCliPresetModel[]) => void;
}

export function QoderCliFormFields({
  providerKey,
  apiKey,
  onApiKeyChange,
  category,
  shouldShowApiKeyLink,
  websiteUrl,
  isPartner,
  partnerPromotionKey,
  models,
  onModelsChange,
}: QoderCliFormFieldsProps) {
  const { t } = useTranslation();
  const preset = getQoderCliPreset(providerKey);
  const availableModels = preset?.models ?? [];
  const availablePlanTypes = getQoderCliSupportedPlanTypes(providerKey);
  const selected = models[0];
  const isCustomModel =
    !!selected && !isQoderCliSupportedModel(providerKey, selected);
  const selectedValue = isCustomModel
    ? CUSTOM_MODEL_VALUE
    : selected
      ? `${selected.type}:${selected.model}`
      : undefined;

  const handleModelChange = (value: string) => {
    if (value === CUSTOM_MODEL_VALUE) {
      if (isCustomModel) {
        return;
      }
      onModelsChange([
        {
          model: "",
          type: availablePlanTypes[0] ?? "pg",
          format: "openai",
          displayName: "",
        },
      ]);
      return;
    }

    const [type, ...modelParts] = value.split(":");
    const modelId = modelParts.join(":");
    const model = availableModels.find(
      (item) => item.type === type && item.model === modelId,
    );
    onModelsChange(model ? [model] : []);
  };

  const updateCustomModel = (
    patch: Partial<Pick<QoderCliPresetModel, "model" | "displayName" | "type">>,
  ) => {
    if (!selected) {
      return;
    }
    onModelsChange([{ ...selected, ...patch }]);
  };

  const handleCustomModelIdChange = (model: string) => {
    updateCustomModel({ model, displayName: model });
  };

  const handleCustomPlanChange = (type: string) => {
    if (availablePlanTypes.includes(type as QoderCliPlanType)) {
      updateCustomModel({ type: type as QoderCliPlanType });
    }
  };

  return (
    <>
      <ApiKeySection
        id="qodercli-api-key"
        value={apiKey}
        onChange={onApiKeyChange}
        category={category}
        shouldShowLink={shouldShowApiKeyLink}
        websiteUrl={websiteUrl}
        isPartner={isPartner}
        partnerPromotionKey={partnerPromotionKey}
      />

      <div className="space-y-2">
        <FormLabel htmlFor="qodercli-model">
          {t("qodercli.model", { defaultValue: "Qoder 模型" })}
          <span className="ml-1 text-destructive">*</span>
        </FormLabel>
        <Select value={selectedValue} onValueChange={handleModelChange}>
          <SelectTrigger id="qodercli-model" className="w-full">
            <SelectValue
              placeholder={t("qodercli.selectModel", {
                defaultValue: "请选择 Qoder 官方支持的模型",
              })}
            />
          </SelectTrigger>
          <SelectContent>
            {availablePlanTypes.map((planType) => (
              <SelectGroup key={planType}>
                <SelectLabel>{getQoderCliPlanLabel(planType)}</SelectLabel>
                {availableModels
                  .filter((model) => model.type === planType)
                  .map((model) => (
                    <SelectItem
                      key={`${model.type}:${model.model}`}
                      value={`${model.type}:${model.model}`}
                    >
                      {getQoderCliModelDisplayLabel(model)}
                    </SelectItem>
                  ))}
              </SelectGroup>
            ))}
            <SelectSeparator />
            <SelectItem value={CUSTOM_MODEL_VALUE}>
              {t("qodercli.addOtherModel", {
                defaultValue: "+ 添加其他模型…",
              })}
            </SelectItem>
          </SelectContent>
        </Select>

        {isCustomModel && selected && (
          <div className="grid gap-3 rounded-lg border border-border-default bg-muted/20 p-3 sm:grid-cols-2">
            <div className="space-y-2">
              <FormLabel htmlFor="qodercli-custom-plan">
                {t("qodercli.planType", { defaultValue: "套餐类型" })}
                <span className="ml-1 text-destructive">*</span>
              </FormLabel>
              <Select
                value={selected.type}
                onValueChange={handleCustomPlanChange}
              >
                <SelectTrigger id="qodercli-custom-plan" className="w-full">
                  <SelectValue
                    placeholder={t("qodercli.selectPlanType", {
                      defaultValue: "请选择套餐类型",
                    })}
                  />
                </SelectTrigger>
                <SelectContent>
                  {availablePlanTypes.map((planType) => (
                    <SelectItem key={planType} value={planType}>
                      {getQoderCliPlanLabel(planType)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>

            <div className="space-y-2">
              <FormLabel htmlFor="qodercli-custom-model-id">
                {t("qodercli.modelId", { defaultValue: "模型 ID" })}
                <span className="ml-1 text-destructive">*</span>
              </FormLabel>
              <Input
                id="qodercli-custom-model-id"
                value={selected.model}
                onChange={(event) =>
                  handleCustomModelIdChange(event.target.value)
                }
                placeholder={t("qodercli.modelIdPlaceholder", {
                  defaultValue: "输入供应商实际支持的模型 ID",
                })}
              />
            </div>

            <p className="text-xs text-muted-foreground sm:col-span-2">
              {t("qodercli.customModelHint", {
                defaultValue:
                  "模型名会原样交给该供应商验证；请填写当前 API Key 实际可用的模型 ID。",
              })}
            </p>
          </div>
        )}

        <p className="text-xs text-muted-foreground">
          {t("qodercli.catalogHint", {
            defaultValue:
              "供应商和套餐类型来自 Qoder 官方 BYOK 目录；可选择官方预设或添加其他模型。",
          })}
        </p>
      </div>
    </>
  );
}
