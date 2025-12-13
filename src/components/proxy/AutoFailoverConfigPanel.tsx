import { useState, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Save, Loader2, Info } from "lucide-react";
import { toast } from "sonner";
import {
  useCircuitBreakerConfig,
  useUpdateCircuitBreakerConfig,
} from "@/lib/query/failover";

export interface AutoFailoverConfigPanelProps {
  enabled: boolean;
  onEnabledChange: (enabled: boolean) => void;
}

export function AutoFailoverConfigPanel({
  enabled,
  onEnabledChange: _onEnabledChange,
}: AutoFailoverConfigPanelProps) {
  // Note: onEnabledChange is currently unused but kept in the interface
  // for potential future use by parent components
  void _onEnabledChange;
  const { t } = useTranslation();
  const { data: config, isLoading, error } = useCircuitBreakerConfig();
  const updateConfig = useUpdateCircuitBreakerConfig();

  const [formData, setFormData] = useState({
    failureThreshold: 5,
    successThreshold: 2,
    timeoutSeconds: 60,
    errorRateThreshold: 0.5,
    minRequests: 10,
  });

  useEffect(() => {
    if (config) {
      setFormData({
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
        ...config,
      });
    }
  };

  if (isLoading) {
    return (
      <div className="flex items-center justify-center p-4">
        <Loader2 className="h-6 w-6 animate-spin text-muted-foreground" />
      </div>
    );
  }

  return (
    <div className="border-0 rounded-none shadow-none bg-transparent">
      {/* Header Switch moved to parent accordion logic or kept here absolutely positioned if styling permits.
            Since we need it in the accordion header, and this component is inside the content, we can use a portal or
            absolute positioning trick similar to ProxyPanel, OR cleaner, just duplicate the switch logic in SettingsPage
            and pass it down. But for now, let's use the absolute positioning trick to "lift" it visually.
            Better yet, let's just render the content directly without the wrapping Card header/collapse logic
            since the user requested "click to expand is detailed info, no need to fold again" (implying the accordion handles folding).
        */}

      <div className="space-y-4">
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
                disabled={!enabled}
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
                disabled={!enabled}
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
                disabled={!enabled}
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
                    errorRateThreshold: (parseInt(e.target.value) || 50) / 100,
                  })
                }
                disabled={!enabled}
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
                disabled={!enabled}
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
            disabled={updateConfig.isPending || !enabled}
          >
            {t("common.reset", "重置")}
          </Button>
          <Button
            onClick={handleSave}
            disabled={updateConfig.isPending || !enabled}
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
                {t("proxy.autoFailover.successThresholdLabel", "恢复成功阈值")}
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
      </div>
    </div>
  );
}
