import { useTranslation } from "react-i18next";
import { FormLabel } from "@/components/ui/form";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import type { ProviderCategory } from "@/types";
import {
  getQoderCliModelDisplayLabel,
  getQoderCliPreset,
  type QoderCliPresetModel,
} from "@/config/qodercliProviderPresets";
import { ApiKeySection } from "./shared";

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
  const selected = models[0];
  const selectedValue = selected
    ? `${selected.type}:${selected.model}`
    : undefined;

  const handleModelChange = (value: string) => {
    const [type, ...modelParts] = value.split(":");
    const modelId = modelParts.join(":");
    const model = availableModels.find(
      (item) => item.type === type && item.model === modelId,
    );
    onModelsChange(model ? [model] : []);
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
            {availableModels.map((model) => (
              <SelectItem
                key={`${model.type}:${model.model}`}
                value={`${model.type}:${model.model}`}
              >
                {getQoderCliModelDisplayLabel(model)}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <p className="text-xs text-muted-foreground">
          {t("qodercli.catalogHint", {
            defaultValue:
              "仅使用 Qoder 官方 BYOK 目录；套餐类型（CP / TP / PG）会随模型一起写入，不能手动修改接口地址或模型名。",
          })}
        </p>
      </div>
    </>
  );
}
