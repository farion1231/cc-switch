import { useTranslation } from "react-i18next";
import { useState, useEffect } from "react";
import { ChevronDown, ChevronRight, Coins, Activity } from "lucide-react";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { cn } from "@/lib/utils";
export type PricingModelSourceOption = "inherit" | "request" | "response";

interface ProviderPricingConfig {
  enabled: boolean;
  costMultiplier?: string;
  pricingModelSource: PricingModelSourceOption;
}

interface ProviderAdvancedConfigProps {
  pricingConfig: ProviderPricingConfig;
  onPricingConfigChange: (config: ProviderPricingConfig) => void;
  testModel: string;
  testPrompt: string;
  onTestModelChange: (value: string) => void;
  onTestPromptChange: (value: string) => void;
}

export function ProviderAdvancedConfig({
  pricingConfig,
  onPricingConfigChange,
  testModel,
  testPrompt,
  onTestModelChange,
  onTestPromptChange,
}: ProviderAdvancedConfigProps) {
  const { t } = useTranslation();
  const [isPricingConfigOpen, setIsPricingConfigOpen] = useState(
    pricingConfig.enabled,
  );
  const [isTestConfigOpen, setIsTestConfigOpen] = useState(
    !!(testModel || testPrompt),
  );

  useEffect(() => {
    setIsPricingConfigOpen(pricingConfig.enabled);
  }, [pricingConfig.enabled]);

  return (
    <div className="space-y-4">
      {/* 计费配置 */}
      <div className="rounded-lg border border-border/50 bg-muted/20">
        <button
          type="button"
          className="flex w-full items-center justify-between p-4 hover:bg-muted/30 transition-colors"
          onClick={() => setIsPricingConfigOpen(!isPricingConfigOpen)}
        >
          <div className="flex items-center gap-3">
            <Coins className="h-4 w-4 text-muted-foreground" />
            <span className="font-medium">
              {t("providerAdvanced.pricingConfig", {
                defaultValue: "计费配置",
              })}
            </span>
          </div>
          <div className="flex items-center gap-3">
            <div
              className="flex items-center gap-2"
              onClick={(e) => e.stopPropagation()}
            >
              <Label
                htmlFor="pricing-config-enabled"
                className="text-sm text-muted-foreground"
              >
                {t("providerAdvanced.useCustomPricing", {
                  defaultValue: "使用单独配置",
                })}
              </Label>
              <Switch
                id="pricing-config-enabled"
                checked={pricingConfig.enabled}
                onCheckedChange={(checked) => {
                  onPricingConfigChange({ ...pricingConfig, enabled: checked });
                  if (checked) setIsPricingConfigOpen(true);
                }}
              />
            </div>
            {isPricingConfigOpen ? (
              <ChevronDown className="h-4 w-4 text-muted-foreground" />
            ) : (
              <ChevronRight className="h-4 w-4 text-muted-foreground" />
            )}
          </div>
        </button>
        <div
          className={cn(
            "overflow-hidden transition-all duration-200",
            isPricingConfigOpen
              ? "max-h-[500px] opacity-100"
              : "max-h-0 opacity-0",
          )}
        >
          <div className="border-t border-border/50 p-4 space-y-4">
            <p className="text-sm text-muted-foreground">
              {t("providerAdvanced.pricingConfigDesc", {
                defaultValue:
                  "为此供应商配置单独的计费参数，不启用时使用全局默认配置。",
              })}
            </p>
            <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
              <div className="space-y-2">
                <Label htmlFor="cost-multiplier">
                  {t("providerAdvanced.costMultiplier", {
                    defaultValue: "成本倍率",
                  })}
                </Label>
                <Input
                  id="cost-multiplier"
                  type="number"
                  step="0.01"
                  min="0"
                  inputMode="decimal"
                  value={pricingConfig.costMultiplier || ""}
                  onChange={(e) =>
                    onPricingConfigChange({
                      ...pricingConfig,
                      costMultiplier: e.target.value || undefined,
                    })
                  }
                  placeholder={t("providerAdvanced.costMultiplierPlaceholder", {
                    defaultValue: "留空使用全局默认（1）",
                  })}
                  disabled={!pricingConfig.enabled}
                />
                <p className="text-xs text-muted-foreground">
                  {t("providerAdvanced.costMultiplierHint", {
                    defaultValue: "实际成本 = 基础成本 × 倍率，支持小数如 1.5",
                  })}
                </p>
              </div>
              <div className="space-y-2">
                <Label htmlFor="pricing-model-source">
                  {t("providerAdvanced.pricingModelSourceLabel", {
                    defaultValue: "计费模式",
                  })}
                </Label>
                <Select
                  value={pricingConfig.pricingModelSource}
                  onValueChange={(value) =>
                    onPricingConfigChange({
                      ...pricingConfig,
                      pricingModelSource: value as PricingModelSourceOption,
                    })
                  }
                  disabled={!pricingConfig.enabled}
                >
                  <SelectTrigger id="pricing-model-source">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="inherit">
                      {t("providerAdvanced.pricingModelSourceInherit", {
                        defaultValue: "继承全局默认",
                      })}
                    </SelectItem>
                    <SelectItem value="request">
                      {t("providerAdvanced.pricingModelSourceRequest", {
                        defaultValue: "请求模型",
                      })}
                    </SelectItem>
                    <SelectItem value="response">
                      {t("providerAdvanced.pricingModelSourceResponse", {
                        defaultValue: "返回模型",
                      })}
                    </SelectItem>
                  </SelectContent>
                </Select>
                <p className="text-xs text-muted-foreground">
                  {t("providerAdvanced.pricingModelSourceHint", {
                    defaultValue: "选择按请求模型还是返回模型进行定价匹配",
                  })}
                </p>
              </div>
            </div>
          </div>
        </div>
      </div>

      {/* 可用性测试专用配置 */}
      <div className="rounded-lg border border-border/50 bg-muted/20">
        <button
          type="button"
          className="flex w-full items-center justify-between p-4 hover:bg-muted/30 transition-colors"
          onClick={() => setIsTestConfigOpen(!isTestConfigOpen)}
        >
          <div className="flex items-center gap-3">
            <Activity className="h-4 w-4 text-muted-foreground" />
            <span className="font-medium">
              {t("providerAdvanced.testConfig", {
                defaultValue: "连通性与可用性测试配置",
              })}
            </span>
          </div>
          <div className="flex items-center gap-3">
            <div
              className="flex items-center gap-2"
              onClick={(e) => e.stopPropagation()}
            >
              <Label
                htmlFor="test-config-enabled"
                className="text-sm text-muted-foreground"
              >
                {t("providerAdvanced.useCustomTestConfig", {
                  defaultValue: "使用专属配置",
                })}
              </Label>
              <Switch
                id="test-config-enabled"
                checked={isTestConfigOpen}
                onCheckedChange={(checked) => {
                  setIsTestConfigOpen(checked);
                  if (!checked) {
                    onTestModelChange("");
                    onTestPromptChange("");
                  }
                }}
              />
            </div>
            {isTestConfigOpen ? (
              <ChevronDown className="h-4 w-4 text-muted-foreground" />
            ) : (
              <ChevronRight className="h-4 w-4 text-muted-foreground" />
            )}
          </div>
        </button>
        <div
          className={cn(
            "overflow-hidden transition-all duration-200",
            isTestConfigOpen
              ? "max-h-[500px] opacity-100"
              : "max-h-0 opacity-0",
          )}
        >
          <div className="border-t border-border/50 p-4 space-y-4">
            <p className="text-sm text-muted-foreground">
              {t("providerAdvanced.testConfigDesc", {
                defaultValue:
                  "为此供应商配置单独的测速测试参数，启用后将覆盖全局的测速模型和提示词。",
              })}
            </p>
            <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
              <div className="space-y-2">
                <Label htmlFor="provider-test-model">
                  {t("providerAdvanced.testModelLabel", {
                    defaultValue: "测试模型名称",
                  })}
                </Label>
                <Input
                  id="provider-test-model"
                  type="text"
                  placeholder="auto (从 /models 获取或使用默认模型)"
                  value={testModel}
                  onChange={(e) => onTestModelChange(e.target.value)}
                  disabled={!isTestConfigOpen}
                />
                <p className="text-xs text-muted-foreground">
                  {t("providerAdvanced.testModelHint", {
                    defaultValue:
                      "支持输入特定模型（如 deepseek-chat 或 qwen-max）。设为 auto 或留空将尝试自适应探测。",
                  })}
                </p>
              </div>
              <div className="space-y-2">
                <Label htmlFor="provider-test-prompt">
                  {t("providerAdvanced.testPromptLabel", {
                    defaultValue: "测试提示词 (Prompt)",
                  })}
                </Label>
                <Input
                  id="provider-test-prompt"
                  type="text"
                  placeholder="hi (测试握手内容)"
                  value={testPrompt}
                  onChange={(e) => onTestPromptChange(e.target.value)}
                  disabled={!isTestConfigOpen}
                />
                <p className="text-xs text-muted-foreground">
                  {t("providerAdvanced.testPromptHint", {
                    defaultValue:
                      "测试时发给该提供商的握手请求文本（如 hi 或 ping）。",
                  })}
                </p>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
