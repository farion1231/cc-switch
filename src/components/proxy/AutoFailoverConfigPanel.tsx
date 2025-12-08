import { useState, useEffect } from "react";
import { useTranslation } from "react-i18next";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { ChevronDown, ChevronRight, Save, Loader2, Info } from "lucide-react";
import { toast } from "sonner";
import {
  useCircuitBreakerConfig,
  useUpdateCircuitBreakerConfig,
} from "@/lib/query/failover";

/**
 * 自动故障转移配置面板
 * 放置在高级设置页面，模型测试配置下方
 */
export function AutoFailoverConfigPanel() {
  const { t } = useTranslation();
  const [isExpanded, setIsExpanded] = useState(false);

  const { data: config, isLoading, error } = useCircuitBreakerConfig();
  const updateConfig = useUpdateCircuitBreakerConfig();

  const [formData, setFormData] = useState({
    enabled: true, // 自动故障转移总开关
    failureThreshold: 5,
    successThreshold: 2,
    timeoutSeconds: 60,
    errorRateThreshold: 0.5,
    minRequests: 10,
  });

  useEffect(() => {
    if (config) {
      setFormData({
        enabled: true, // 默认开启，后续可以从数据库读取
        ...config,
      });
    }
  }, [config]);

  const handleSave = async () => {
    try {
      await updateConfig.mutateAsync({
        failureThreshold: formData.failureThreshold,
        successThreshold: formData.successThreshold,
        timeoutSeconds: formData.timeoutSeconds,
        errorRateThreshold: formData.errorRateThreshold,
        minRequests: formData.minRequests,
      });
      toast.success(
        t("proxy.autoFailover.configSaved", "自动故障转移配置已保存"),
      );
    } catch (e) {
      toast.error(
        t("proxy.autoFailover.configSaveFailed", "保存失败") + ": " + String(e),
      );
    }
  };

  const handleReset = () => {
    if (config) {
      setFormData({
        enabled: formData.enabled,
        ...config,
      });
    }
  };

  if (isLoading) {
    return (
      <Card className="border rounded-lg">
        <CardHeader
          className="cursor-pointer"
          onClick={() => setIsExpanded(!isExpanded)}
        >
          <div className="flex items-center gap-2">
            <ChevronRight className="h-4 w-4" />
            <CardTitle className="text-base">
              {t("proxy.autoFailover.title", "自动故障转移配置")}
            </CardTitle>
          </div>
        </CardHeader>
      </Card>
    );
  }

  return (
    <Card className="border rounded-lg">
      <CardHeader
        className="cursor-pointer select-none"
        onClick={() => setIsExpanded(!isExpanded)}
      >
        <div className="flex items-center gap-2">
          {isExpanded ? (
            <ChevronDown className="h-4 w-4 text-muted-foreground" />
          ) : (
            <ChevronRight className="h-4 w-4 text-muted-foreground" />
          )}
          <div className="flex-1">
            <div className="flex items-center gap-3">
              <CardTitle className="text-base">
                {t("proxy.autoFailover.title", "自动故障转移配置")}
              </CardTitle>
              {/* 总开关 - 点击时阻止折叠/展开 */}
              <div
                onClick={(e) => e.stopPropagation()}
                className="flex items-center gap-2"
              >
                <Switch
                  id="autoFailoverEnabled"
                  checked={formData.enabled}
                  onCheckedChange={(checked) =>
                    setFormData({ ...formData, enabled: checked })
                  }
                  className="scale-90"
                />
                <Label
                  htmlFor="autoFailoverEnabled"
                  className="text-xs font-medium cursor-pointer"
                >
                  {formData.enabled
                    ? t("proxy.autoFailover.enabled", "已启用")
                    : t("proxy.autoFailover.disabled", "已禁用")}
                </Label>
              </div>
            </div>
            {!isExpanded && (
              <CardDescription className="mt-1">
                {t(
                  "proxy.autoFailover.description",
                  "配置多个代理目标之间的自动切换规则和熔断策略",
                )}
              </CardDescription>
            )}
          </div>
        </div>
      </CardHeader>

      {isExpanded && (
        <CardContent className="space-y-4">
          {error && (
            <Alert variant="destructive">
              <AlertDescription>{String(error)}</AlertDescription>
            </Alert>
          )}

          <Alert className="border-blue-500/40 bg-blue-500/10">
            <Info className="h-4 w-4" />
            <AlertDescription className="text-sm">
              {t(
                "proxy.autoFailover.info",
                "当启用多个代理目标时，系统会按优先级顺序依次尝试。当某个供应商连续失败达到阈值时，熔断器会自动打开，跳过该供应商。",
              )}
            </AlertDescription>
          </Alert>

          {/* 重试与超时配置 */}
          <div className="space-y-4 rounded-lg border border-white/10 bg-muted/30 p-4">
            <h4 className="text-sm font-semibold">
              {t("proxy.autoFailover.retrySettings", "重试与超时设置")}
            </h4>

            <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
              <div className="space-y-2">
                <Label htmlFor="failureThreshold">
                  {t("proxy.autoFailover.failureThreshold", "失败阈值")}
                </Label>
                <Input
                  id="failureThreshold"
                  type="number"
                  min="1"
                  max="20"
                  value={formData.failureThreshold}
                  onChange={(e) =>
                    setFormData({
                      ...formData,
                      failureThreshold: parseInt(e.target.value) || 5,
                    })
                  }
                  disabled={!formData.enabled}
                />
                <p className="text-xs text-muted-foreground">
                  {t(
                    "proxy.autoFailover.failureThresholdHint",
                    "连续失败多少次后打开熔断器（建议: 3-10）",
                  )}
                </p>
              </div>

              <div className="space-y-2">
                <Label htmlFor="timeoutSeconds">
                  {t("proxy.autoFailover.timeout", "恢复等待时间（秒）")}
                </Label>
                <Input
                  id="timeoutSeconds"
                  type="number"
                  min="10"
                  max="300"
                  value={formData.timeoutSeconds}
                  onChange={(e) =>
                    setFormData({
                      ...formData,
                      timeoutSeconds: parseInt(e.target.value) || 60,
                    })
                  }
                  disabled={!formData.enabled}
                />
                <p className="text-xs text-muted-foreground">
                  {t(
                    "proxy.autoFailover.timeoutHint",
                    "熔断器打开后，等待多久后尝试恢复（建议: 30-120）",
                  )}
                </p>
              </div>
            </div>
          </div>

          {/* 熔断器高级配置 */}
          <div className="space-y-4 rounded-lg border border-white/10 bg-muted/30 p-4">
            <h4 className="text-sm font-semibold">
              {t("proxy.autoFailover.circuitBreakerSettings", "熔断器高级设置")}
            </h4>

            <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
              <div className="space-y-2">
                <Label htmlFor="successThreshold">
                  {t("proxy.autoFailover.successThreshold", "恢复成功阈值")}
                </Label>
                <Input
                  id="successThreshold"
                  type="number"
                  min="1"
                  max="10"
                  value={formData.successThreshold}
                  onChange={(e) =>
                    setFormData({
                      ...formData,
                      successThreshold: parseInt(e.target.value) || 2,
                    })
                  }
                  disabled={!formData.enabled}
                />
                <p className="text-xs text-muted-foreground">
                  {t(
                    "proxy.autoFailover.successThresholdHint",
                    "半开状态下成功多少次后关闭熔断器",
                  )}
                </p>
              </div>

              <div className="space-y-2">
                <Label htmlFor="errorRateThreshold">
                  {t("proxy.autoFailover.errorRate", "错误率阈值 (%)")}
                </Label>
                <Input
                  id="errorRateThreshold"
                  type="number"
                  min="0"
                  max="100"
                  step="5"
                  value={Math.round(formData.errorRateThreshold * 100)}
                  onChange={(e) =>
                    setFormData({
                      ...formData,
                      errorRateThreshold:
                        (parseInt(e.target.value) || 50) / 100,
                    })
                  }
                  disabled={!formData.enabled}
                />
                <p className="text-xs text-muted-foreground">
                  {t(
                    "proxy.autoFailover.errorRateHint",
                    "错误率超过此值时打开熔断器",
                  )}
                </p>
              </div>

              <div className="space-y-2">
                <Label htmlFor="minRequests">
                  {t("proxy.autoFailover.minRequests", "最小请求数")}
                </Label>
                <Input
                  id="minRequests"
                  type="number"
                  min="5"
                  max="100"
                  value={formData.minRequests}
                  onChange={(e) =>
                    setFormData({
                      ...formData,
                      minRequests: parseInt(e.target.value) || 10,
                    })
                  }
                  disabled={!formData.enabled}
                />
                <p className="text-xs text-muted-foreground">
                  {t(
                    "proxy.autoFailover.minRequestsHint",
                    "计算错误率前的最小请求数",
                  )}
                </p>
              </div>
            </div>
          </div>

          {/* 操作按钮 */}
          <div className="flex justify-end gap-3 pt-2">
            <Button
              variant="outline"
              onClick={handleReset}
              disabled={updateConfig.isPending || !formData.enabled}
            >
              {t("common.reset", "重置")}
            </Button>
            <Button
              onClick={handleSave}
              disabled={updateConfig.isPending || !formData.enabled}
            >
              {updateConfig.isPending ? (
                <>
                  <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                  {t("common.saving", "保存中...")}
                </>
              ) : (
                <>
                  <Save className="mr-2 h-4 w-4" />
                  {t("common.save", "保存")}
                </>
              )}
            </Button>
          </div>

          {/* 说明信息 */}
          <div className="p-4 bg-muted/50 rounded-lg space-y-2 text-sm">
            <h4 className="font-medium">
              {t("proxy.autoFailover.explanationTitle", "工作原理")}
            </h4>
            <ul className="space-y-1 text-muted-foreground">
              <li>
                •{" "}
                <strong>
                  {t("proxy.autoFailover.failureThresholdLabel", "失败阈值")}
                </strong>
                ：
                {t(
                  "proxy.autoFailover.failureThresholdExplain",
                  "连续失败达到此次数时，熔断器打开，该供应商暂时不可用",
                )}
              </li>
              <li>
                •{" "}
                <strong>
                  {t("proxy.autoFailover.timeoutLabel", "恢复等待时间")}
                </strong>
                ：
                {t(
                  "proxy.autoFailover.timeoutExplain",
                  "熔断器打开后，等待此时间后尝试半开状态",
                )}
              </li>
              <li>
                •{" "}
                <strong>
                  {t(
                    "proxy.autoFailover.successThresholdLabel",
                    "恢复成功阈值",
                  )}
                </strong>
                ：
                {t(
                  "proxy.autoFailover.successThresholdExplain",
                  "半开状态下，成功达到此次数时关闭熔断器，供应商恢复可用",
                )}
              </li>
              <li>
                •{" "}
                <strong>
                  {t("proxy.autoFailover.errorRateLabel", "错误率阈值")}
                </strong>
                ：
                {t(
                  "proxy.autoFailover.errorRateExplain",
                  "错误率超过此值时，即使未达到失败阈值也会打开熔断器",
                )}
              </li>
            </ul>
          </div>
        </CardContent>
      )}
    </Card>
  );
}
