import { useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "@/lib/toast";
import { Save, Plus, Globe } from "lucide-react";
import { FullScreenPanel } from "@/components/common/FullScreenPanel";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { useUpdateModelPricing } from "@/lib/query/usage";
import {
  isNonNegativeDecimalString,
  type ModelPricing,
  type PromptTierPricing,
} from "@/types/usage";
import { ModelsDevPickerDialog } from "./ModelsDevPickerDialog";

interface PricingEditModalProps {
  open: boolean;
  model: ModelPricing;
  isNew?: boolean;
  onClose: () => void;
}

const PRICE_INPUT_STEP = "0.0001";

// 提示词超过 100K 时的四项单价；标签沿用上方标准价的文案
const TIER_FIELDS = [
  {
    key: "inputCostPerMillion",
    id: "inputCostAbove100k",
    labelKey: "usage.inputCostPerMillion",
    label: "输入成本 (每百万 tokens, USD)",
  },
  {
    key: "outputCostPerMillion",
    id: "outputCostAbove100k",
    labelKey: "usage.outputCostPerMillion",
    label: "输出成本 (每百万 tokens, USD)",
  },
  {
    key: "cacheReadCostPerMillion",
    id: "cacheReadCostAbove100k",
    labelKey: "usage.cacheReadCostPerMillion",
    label: "缓存读取成本 (每百万 tokens, USD)",
  },
  {
    key: "cacheCreationCostPerMillion",
    id: "cacheCreationCostAbove100k",
    labelKey: "usage.cacheCreationCostPerMillion",
    label: "缓存写入成本 (每百万 tokens, USD)",
  },
] as const satisfies ReadonlyArray<{
  key: keyof PromptTierPricing;
  id: string;
  labelKey: string;
  label: string;
}>;

const EMPTY_TIER: PromptTierPricing = {
  inputCostPerMillion: "",
  outputCostPerMillion: "",
  cacheReadCostPerMillion: "",
  cacheCreationCostPerMillion: "",
};

export function PricingEditModal({
  open,
  model,
  isNew = false,
  onClose,
}: PricingEditModalProps) {
  const { t } = useTranslation();
  const updatePricing = useUpdateModelPricing();
  const [isPickerOpen, setIsPickerOpen] = useState(false);

  const [formData, setFormData] = useState({
    modelId: model.modelId,
    displayName: model.displayName,
    inputCost: model.inputCostPerMillion,
    outputCost: model.outputCostPerMillion,
    cacheReadCost: model.cacheReadCostPerMillion,
    cacheCreationCost: model.cacheCreationCostPerMillion,
  });
  const [tier, setTier] = useState<PromptTierPricing>(
    model.above100k ?? EMPTY_TIER,
  );

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
      if (!isNonNegativeDecimalString(value)) {
        toast.error(t("usage.invalidPrice", "价格必须为非负数"));
        return;
      }
    }

    // 超过 100K 的单价：四项要么都填，要么都空
    const filledTiers = TIER_FIELDS.filter(({ key }) => tier[key].trim());
    if (filledTiers.length > 0 && filledTiers.length < TIER_FIELDS.length) {
      toast.error(
        t(
          "usage.above100kIncomplete",
          "超过 100K 的四项单价要么都填，要么都留空",
        ),
      );
      return;
    }
    if (filledTiers.some(({ key }) => !isNonNegativeDecimalString(tier[key]))) {
      toast.error(t("usage.invalidPrice", "价格必须为非负数"));
      return;
    }
    // 四项都空时：原来有高价档就提交 null 清除；原来没有就不带这个字段，后端保持原样
    const above100k =
      filledTiers.length > 0 ? tier : model.above100k ? null : undefined;

    try {
      await updatePricing.mutateAsync({
        modelId: isNew ? formData.modelId : model.modelId,
        displayName: formData.displayName,
        inputCostPerMillion: formData.inputCost,
        outputCostPerMillion: formData.outputCost,
        cacheReadCostPerMillion: formData.cacheReadCost,
        cacheCreationCostPerMillion: formData.cacheCreationCost,
        ...(above100k !== undefined && { above100k }),
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
      trackUnsavedChanges
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
          variant="solid"
          size="regular"
          disabled={updatePricing.isPending}
        >
          {isNew ? <Plus className="h-4 w-4" /> : <Save className="h-4 w-4" />}
          {updatePricing.isPending
            ? t("common.saving", "保存中...")
            : isNew
              ? t("common.add", "新增")
              : t("common.save", "保存")}
        </Button>
      }
    >
      {isNew && (
        <div className="mb-6 flex items-center justify-between gap-3 rounded-panel border border-border bg-subtle px-3 py-2.5">
          <p className="text-caption text-fg-2">
            {t(
              "usage.modelsDevHint",
              "无需手动填写，可从 models.dev 选择模型定价",
            )}
          </p>
          <Button
            type="button"
            variant="neutral"
            size="compact"
            onClick={() => setIsPickerOpen(true)}
            className="shrink-0"
          >
            <Globe className="h-3.5 w-3.5" />
            {t("usage.importFromModelsDev", "从 models.dev 导入")}
          </Button>
        </div>
      )}

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
            step={PRICE_INPUT_STEP}
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
            step={PRICE_INPUT_STEP}
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
            step={PRICE_INPUT_STEP}
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
            step={PRICE_INPUT_STEP}
            min="0"
            value={formData.cacheCreationCost}
            onChange={(e) =>
              setFormData({ ...formData, cacheCreationCost: e.target.value })
            }
            required
          />
        </div>

        <div
          role="group"
          aria-labelledby="above100k-hint"
          className="space-y-4 rounded-panel border border-border bg-subtle p-3"
        >
          <p id="above100k-hint" className="text-caption text-fg-2">
            {t(
              "usage.above100kHint",
              "提示词超过 100K tokens（含缓存读写）时的单价。四项都留空表示不分档。",
            )}
          </p>
          {TIER_FIELDS.map(({ key, id, labelKey, label }) => (
            <div key={id} className="space-y-2">
              <Label htmlFor={id}>{t(labelKey, label)}</Label>
              <Input
                id={id}
                type="number"
                step={PRICE_INPUT_STEP}
                min="0"
                value={tier[key]}
                onChange={(e) => setTier({ ...tier, [key]: e.target.value })}
              />
            </div>
          ))}
        </div>
      </form>

      {isNew && isPickerOpen && (
        <ModelsDevPickerDialog
          open={isPickerOpen}
          onClose={() => setIsPickerOpen(false)}
          onImported={() => {
            setIsPickerOpen(false);
            onClose();
          }}
        />
      )}
    </FullScreenPanel>
  );
}
