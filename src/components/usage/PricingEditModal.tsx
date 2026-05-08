import { useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Save, Plus } from "lucide-react";
import { FullScreenPanel } from "@/components/common/FullScreenPanel";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { useUpdateModelPricing } from "@/lib/query/usage";
import type { ModelPricing } from "@/types/usage";

interface PricingEditModalProps {
  open: boolean;
  model: ModelPricing;
  isNew?: boolean;
  onClose: () => void;
}

export function PricingEditModal({
  open,
  model,
  isNew = false,
  onClose,
}: PricingEditModalProps) {
  const { t } = useTranslation();
  const updatePricing = useUpdateModelPricing();

  const [formData, setFormData] = useState({
    modelId: model.modelId,
    displayName: model.displayName,
    inputCost: model.inputCostPerMillion,
    outputCost: model.outputCostPerMillion,
    cacheReadCost: model.cacheReadCostPerMillion,
    cacheCreationCost: model.cacheCreationCostPerMillion,
  });

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();

    // 验证模型 ID
    if (isNew && !formData.modelId.trim()) {
      toast.error(t("usage.modelIdRequired", "模型 ID 不能为空"));
      return;
    }

    // 验证非负数
    const values = [
      formData.inputCost,
      formData.outputCost,
      formData.cacheReadCost,
      formData.cacheCreationCost,
    ];

    for (const value of values) {
      const num = parseFloat(value);
      if (isNaN(num) || num < 0) {
        toast.error(t("usage.invalidPrice", "价格必须为非负数"));
        return;
      }
    }

    try {
      await updatePricing.mutateAsync({
        modelId: isNew ? formData.modelId : model.modelId,
        displayName: formData.displayName,
        inputCost: formData.inputCost,
        outputCost: formData.outputCost,
        cacheReadCost: formData.cacheReadCost,
        cacheCreationCost: formData.cacheCreationCost,
      });

      toast.success(
        isNew
          ? t("usage.pricingAdded", "定价已添加")
          : t("usage.pricingUpdated", "定价已更新"),
        { closeButton: true },
      );

      onClose();
    } catch (error) {
      toast.error(String(error));
    }
  };

  return (
    <FullScreenPanel
      isOpen={open}
      title={
        isNew
          ? t("usage.addPricing", "新增定价")
          : `${t("usage.editPricing", "编辑定价")} - ${model.modelId}`
      }
      onClose={onClose}
      footer={
        <Button
          type="submit"
          form="pricing-form"
          disabled={updatePricing.isPending}
        >
          {isNew ? (
            <Plus className="h-4 w-4 mr-2" />
          ) : (
            <Save className="h-4 w-4 mr-2" />
          )}
          {updatePricing.isPending
            ? t("common.saving", "保存中...")
            : isNew
              ? t("common.add", "新增")
              : t("common.save", "保存")}
        </Button>
      }
    >
      <form id="pricing-form" onSubmit={handleSubmit} className="space-y-6">
        {isNew && (
          <div className="space-y-2">
            <Label htmlFor="modelId">{t("usage.modelId", "模型 ID")}</Label>
            <Input
              id="modelId"
              value={formData.modelId}
              onChange={(e) =>
                setFormData({ ...formData, modelId: e.target.value })
              }
              placeholder={t("usage.modelIdPlaceholder", {
                defaultValue: "例如: claude-3-5-sonnet-20241022",
              })}
              required
            />
          </div>
        )}

        <div className="space-y-2">
          <Label htmlFor="displayName">
            {t("usage.displayName", "显示名称")}
          </Label>
          <Input
            id="displayName"
            value={formData.displayName}
            onChange={(e) =>
              setFormData({ ...formData, displayName: e.target.value })
            }
            placeholder={t("usage.displayNamePlaceholder", {
              defaultValue: "例如: Claude 3.5 Sonnet",
            })}
            required
          />
        </div>

        <div className="space-y-2">
          <Label htmlFor="inputCost">
            {t("usage.inputCostPerMillion", "输入成本 (每百万 tokens, USD)")}
          </Label>
          <Input
            id="inputCost"
            type="number"
            step="0.01"
            min="0"
            value={formData.inputCost}
            onChange={(e) =>
              setFormData({ ...formData, inputCost: e.target.value })
            }
            required
          />
        </div>

        <div className="space-y-2">
          <Label htmlFor="outputCost">
            {t("usage.outputCostPerMillion", "输出成本 (每百万 tokens, USD)")}
          </Label>
          <Input
            id="outputCost"
            type="number"
            step="0.01"
            min="0"
            value={formData.outputCost}
            onChange={(e) =>
              setFormData({ ...formData, outputCost: e.target.value })
            }
            required
          />
        </div>

        <div className="space-y-2">
          <Label htmlFor="cacheReadCost">
            {t(
              "usage.cacheReadCostPerMillion",
              "缓存读取成本 (每百万 tokens, USD)",
            )}
          </Label>
          <Input
            id="cacheReadCost"
            type="number"
            step="0.01"
            min="0"
            value={formData.cacheReadCost}
            onChange={(e) =>
              setFormData({ ...formData, cacheReadCost: e.target.value })
            }
            required
          />
        </div>

        <div className="space-y-2">
          <Label htmlFor="cacheCreationCost">
            {t(
              "usage.cacheCreationCostPerMillion",
              "缓存写入成本 (每百万 tokens, USD)",
            )}
          </Label>
          <Input
            id="cacheCreationCost"
            type="number"
            step="0.01"
            min="0"
            value={formData.cacheCreationCost}
            onChange={(e) =>
              setFormData({ ...formData, cacheCreationCost: e.target.value })
            }
            required
          />
        </div>
      </form>
    </FullScreenPanel>
  );
}
