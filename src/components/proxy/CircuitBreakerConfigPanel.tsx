import {
  useCircuitBreakerConfig,
  useUpdateCircuitBreakerConfig,
} from "@/lib/query/failover";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Button } from "@/components/ui/button";
import { useState, useEffect } from "react";
import { toast } from "sonner";

/**
 * 熔断器配置面板
 * 允许用户调整熔断器参数
 */
export function CircuitBreakerConfigPanel() {
  const { data: config, isLoading } = useCircuitBreakerConfig();
  const updateConfig = useUpdateCircuitBreakerConfig();

  const [formData, setFormData] = useState({
    failureThreshold: 5,
    successThreshold: 2,
    timeoutSeconds: 60,
    errorRateThreshold: 0.5,
    minRequests: 10,
  });

  // 当配置加载完成时更新表单数据
  useEffect(() => {
    if (config) {
      setFormData(config);
    }
  }, [config]);

  const handleSave = async () => {
    try {
      await updateConfig.mutateAsync(formData);
      toast.success("熔断器配置已保存", { closeButton: true });
    } catch (error) {
      toast.error("保存失败: " + String(error));
    }
  };

  const handleReset = () => {
    if (config) {
      setFormData(config);
    }
  };

  if (isLoading) {
    return <div className="text-sm text-muted-foreground">加载中...</div>;
  }

  return (
    <div className="space-y-6">
      <div>
        <h3 className="text-lg font-semibold">熔断器配置</h3>
        <p className="text-sm text-muted-foreground mt-1">
          调整熔断器参数以控制故障检测和恢复行为
        </p>
      </div>

      <div className="h-px bg-border my-4" />

      <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
        {/* 失败阈值 */}
        <div className="space-y-2">
          <Label htmlFor="failureThreshold">失败阈值</Label>
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
          />
          <p className="text-xs text-muted-foreground">
            连续失败多少次后打开熔断器
          </p>
        </div>

        {/* 超时时间 */}
        <div className="space-y-2">
          <Label htmlFor="timeoutSeconds">超时时间（秒）</Label>
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
          />
          <p className="text-xs text-muted-foreground">
            熔断器打开后多久尝试恢复（半开状态）
          </p>
        </div>

        {/* 成功阈值 */}
        <div className="space-y-2">
          <Label htmlFor="successThreshold">成功阈值</Label>
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
          />
          <p className="text-xs text-muted-foreground">
            半开状态下成功多少次后关闭熔断器
          </p>
        </div>

        {/* 错误率阈值 */}
        <div className="space-y-2">
          <Label htmlFor="errorRateThreshold">错误率阈值 (%)</Label>
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
          />
          <p className="text-xs text-muted-foreground">
            错误率超过此值时打开熔断器
          </p>
        </div>

        {/* 最小请求数 */}
        <div className="space-y-2">
          <Label htmlFor="minRequests">最小请求数</Label>
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
          />
          <p className="text-xs text-muted-foreground">
            计算错误率前的最小请求数
          </p>
        </div>
      </div>

      <div className="flex gap-3">
        <Button onClick={handleSave} disabled={updateConfig.isPending}>
          {updateConfig.isPending ? "保存中..." : "保存配置"}
        </Button>
        <Button
          variant="outline"
          onClick={handleReset}
          disabled={updateConfig.isPending}
        >
          重置
        </Button>
      </div>

      {/* 说明信息 */}
      <div className="p-4 bg-muted/50 rounded-lg space-y-2 text-sm">
        <h4 className="font-medium">配置说明</h4>
        <ul className="space-y-1 text-muted-foreground">
          <li>
            • <strong>失败阈值</strong>：连续失败达到此次数时，熔断器打开
          </li>
          <li>
            • <strong>超时时间</strong>：熔断器打开后，等待此时间后尝试半开
          </li>
          <li>
            • <strong>成功阈值</strong>：半开状态下，成功达到此次数时关闭熔断器
          </li>
          <li>
            • <strong>错误率阈值</strong>：错误率超过此值时，熔断器打开
          </li>
          <li>
            • <strong>最小请求数</strong>：只有请求数达到此值后才计算错误率
          </li>
        </ul>
      </div>
    </div>
  );
}
