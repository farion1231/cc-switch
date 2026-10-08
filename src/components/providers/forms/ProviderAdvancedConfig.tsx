import { useTranslation } from "react-i18next";
import { useState, useEffect } from "react";
import { ChevronDown, ChevronRight, Gauge, Plus, X } from "lucide-react";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { Button } from "@/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { cn } from "@/lib/utils";
import { CODING_PLAN_CONCURRENCY_PRESETS } from "@/config/codingPlanPresets";

/** 并发上限表单态（字符串以支持清空输入框） */
export interface ProviderLoadBalanceFormConfig {
  enabled: boolean;
  /** 账号默认并发上限；空串 = 不限 */
  maxConcurrentRequests: string;
  /** 按模型覆盖行 */
  byModel: Array<{ model: string; limit: string }>;
}

interface ProviderAdvancedConfigProps {
  loadBalanceConfig: ProviderLoadBalanceFormConfig;
  onLoadBalanceConfigChange: (config: ProviderLoadBalanceFormConfig) => void;
}

/** 供应商编辑页的并发上限折叠区（会话粘性负载均衡的每账号上限）。 */
export function ProviderAdvancedConfig({
  loadBalanceConfig,
  onLoadBalanceConfigChange,
}: ProviderAdvancedConfigProps) {
  const { t } = useTranslation();
  const [isLoadBalanceOpen, setIsLoadBalanceOpen] = useState(
    loadBalanceConfig.enabled,
  );

  useEffect(() => {
    setIsLoadBalanceOpen(loadBalanceConfig.enabled);
  }, [loadBalanceConfig.enabled]);

  /** 套餐预设选中：一键填充默认值与模型覆盖 */
  const applyConcurrencyPreset = (presetId: string) => {
    const preset = CODING_PLAN_CONCURRENCY_PRESETS.find(
      (p) => p.id === presetId,
    );
    if (!preset) return;
    onLoadBalanceConfigChange({
      ...loadBalanceConfig,
      enabled: true,
      maxConcurrentRequests:
        preset.maxConcurrentRequests === undefined
          ? ""
          : String(preset.maxConcurrentRequests),
      byModel: Object.entries(preset.maxConcurrentByModel ?? {}).map(
        ([model, limit]) => ({ model, limit: String(limit) }),
      ),
    });
  };

  return (
    <div className="space-y-4">
      <div className="rounded-lg border border-border/50 bg-muted/20">
        <button
          type="button"
          className="flex w-full items-center justify-between p-4 hover:bg-muted/30 transition-colors"
          onClick={() => setIsLoadBalanceOpen(!isLoadBalanceOpen)}
        >
          <div className="flex items-center gap-3">
            <Gauge className="h-4 w-4 text-muted-foreground" />
            <span className="font-medium">
              {t("providerAdvanced.loadBalance.title", {
                defaultValue: "并发上限",
              })}
            </span>
          </div>
          <div className="flex items-center gap-3">
            <div
              className="flex items-center gap-2"
              onClick={(e) => e.stopPropagation()}
            >
              <Label
                htmlFor="load-balance-enabled"
                className="text-sm text-muted-foreground"
              >
                {t("providerAdvanced.loadBalance.enable", {
                  defaultValue: "启用",
                })}
              </Label>
              <Switch
                id="load-balance-enabled"
                checked={loadBalanceConfig.enabled}
                onCheckedChange={(checked) => {
                  onLoadBalanceConfigChange({
                    ...loadBalanceConfig,
                    enabled: checked,
                  });
                  if (checked) setIsLoadBalanceOpen(true);
                }}
              />
            </div>
            {isLoadBalanceOpen ? (
              <ChevronDown className="h-4 w-4 text-muted-foreground" />
            ) : (
              <ChevronRight className="h-4 w-4 text-muted-foreground" />
            )}
          </div>
        </button>
        <div
          className={cn(
            "overflow-hidden transition-all duration-200",
            isLoadBalanceOpen
              ? "max-h-[800px] opacity-100"
              : "max-h-0 opacity-0",
          )}
        >
          <div className="border-t border-border/50 p-4 space-y-4">
            <p className="text-sm text-muted-foreground">
              {t("providerAdvanced.loadBalance.hint", {
                defaultValue:
                  "会话粘性负载均衡生效时，新会话优先分配到未达上限的账号；0 或留空 = 不限。粘性命中的会话不受上限约束。",
              })}
            </p>
            <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
              <div className="space-y-2">
                <Label htmlFor="load-balance-preset">
                  {t("providerAdvanced.loadBalance.preset", {
                    defaultValue: "套餐预设",
                  })}
                </Label>
                <Select
                  value=""
                  onValueChange={applyConcurrencyPreset}
                  disabled={!loadBalanceConfig.enabled}
                >
                  <SelectTrigger id="load-balance-preset">
                    <SelectValue
                      placeholder={t("codingPlanPresets.placeholder", {
                        defaultValue: "套餐预设（一键填充）",
                      })}
                    />
                  </SelectTrigger>
                  <SelectContent>
                    {CODING_PLAN_CONCURRENCY_PRESETS.map((preset) => (
                      <SelectItem key={preset.id} value={preset.id}>
                        {t(preset.labelKey)}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
                <p className="text-xs text-muted-foreground">
                  {t("providerAdvanced.loadBalance.presetHint", {
                    defaultValue: "选中即填充，来源见各档位说明，可手改",
                  })}
                </p>
              </div>
              <div className="space-y-2">
                <Label htmlFor="load-balance-max-concurrent">
                  {t("providerAdvanced.loadBalance.maxConcurrent", {
                    defaultValue: "账号默认并发上限",
                  })}
                </Label>
                <Input
                  id="load-balance-max-concurrent"
                  type="number"
                  min="0"
                  inputMode="numeric"
                  value={loadBalanceConfig.maxConcurrentRequests}
                  onChange={(e) =>
                    onLoadBalanceConfigChange({
                      ...loadBalanceConfig,
                      maxConcurrentRequests: e.target.value,
                    })
                  }
                  placeholder={t(
                    "providerAdvanced.loadBalance.maxConcurrentHint",
                    { defaultValue: "0 或留空 = 不限" },
                  )}
                  disabled={!loadBalanceConfig.enabled}
                />
              </div>
            </div>
            <div className="space-y-2">
              <Label>
                {t("providerAdvanced.loadBalance.byModel", {
                  defaultValue: "按模型覆盖",
                })}
              </Label>
              <p className="text-xs text-muted-foreground">
                {t("providerAdvanced.loadBalance.byModelHint", {
                  defaultValue:
                    "针对指定出站模型名单独设限，优先于账号默认值（模型映射后的上游真名）",
                })}
              </p>
              {loadBalanceConfig.byModel.map((row, index) => (
                <div key={index} className="flex items-center gap-2">
                  <Input
                    type="text"
                    value={row.model}
                    placeholder={t("providerAdvanced.loadBalance.model", {
                      defaultValue: "模型名",
                    })}
                    className="flex-1"
                    disabled={!loadBalanceConfig.enabled}
                    onChange={(e) => {
                      const byModel = [...loadBalanceConfig.byModel];
                      byModel[index] = { ...row, model: e.target.value };
                      onLoadBalanceConfigChange({
                        ...loadBalanceConfig,
                        byModel,
                      });
                    }}
                  />
                  <Input
                    type="number"
                    min="1"
                    inputMode="numeric"
                    value={row.limit}
                    placeholder={t("providerAdvanced.loadBalance.limit", {
                      defaultValue: "上限",
                    })}
                    className="w-28"
                    disabled={!loadBalanceConfig.enabled}
                    onChange={(e) => {
                      const byModel = [...loadBalanceConfig.byModel];
                      byModel[index] = { ...row, limit: e.target.value };
                      onLoadBalanceConfigChange({
                        ...loadBalanceConfig,
                        byModel,
                      });
                    }}
                  />
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon"
                    className="h-8 w-8 shrink-0"
                    disabled={!loadBalanceConfig.enabled}
                    onClick={() => {
                      const byModel = loadBalanceConfig.byModel.filter(
                        (_, i) => i !== index,
                      );
                      onLoadBalanceConfigChange({
                        ...loadBalanceConfig,
                        byModel,
                      });
                    }}
                    aria-label={t("common.delete", { defaultValue: "删除" })}
                  >
                    <X className="h-4 w-4" />
                  </Button>
                </div>
              ))}
              <Button
                type="button"
                variant="outline"
                size="sm"
                disabled={!loadBalanceConfig.enabled}
                onClick={() =>
                  onLoadBalanceConfigChange({
                    ...loadBalanceConfig,
                    byModel: [
                      ...loadBalanceConfig.byModel,
                      { model: "", limit: "" },
                    ],
                  })
                }
              >
                <Plus className="h-4 w-4 mr-1" />
                {t("providerAdvanced.loadBalance.addOverride", {
                  defaultValue: "添加模型覆盖",
                })}
              </Button>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
