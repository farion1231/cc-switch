import { useState, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Save, Loader2, Info } from "lucide-react";
import { toast } from "sonner";
import {
  getStreamCheckConfig,
  saveStreamCheckConfig,
  type StreamCheckConfig,
} from "@/lib/api/connectivity-check";

export function ConnectivityCheckConfigPanel() {
  const { t } = useTranslation();
  const [isLoading, setIsLoading] = useState(true);
  const [isSaving, setIsSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // 使用字符串状态以支持完全清空数字输入框
  const [config, setConfig] = useState({
    timeoutSecs: "8",
    maxRetries: "1",
    degradedThresholdMs: "6000",
  });

  useEffect(() => {
    loadConfig();
  }, []);

  async function loadConfig() {
    try {
      setIsLoading(true);
      setError(null);
      const data = await getStreamCheckConfig();
      setConfig({
        timeoutSecs: String(data.timeoutSecs),
        maxRetries: String(data.maxRetries),
        degradedThresholdMs: String(data.degradedThresholdMs),
      });
    } catch (e) {
      setError(String(e));
    } finally {
      setIsLoading(false);
    }
  }

  async function handleSave() {
    // 解析数字，空值使用默认值，0 是有效值
    const parseNum = (val: string, defaultVal: number) => {
      const n = parseInt(val);
      return isNaN(n) ? defaultVal : n;
    };
    try {
      setIsSaving(true);
      const parsed: StreamCheckConfig = {
        timeoutSecs: parseNum(config.timeoutSecs, 8),
        maxRetries: parseNum(config.maxRetries, 1),
        degradedThresholdMs: parseNum(config.degradedThresholdMs, 6000),
      };
      await saveStreamCheckConfig(parsed);
      toast.success(t("streamCheck.configSaved"), {
        closeButton: true,
      });
    } catch (e) {
      toast.error(t("streamCheck.configSaveFailed") + ": " + String(e));
    } finally {
      setIsSaving(false);
    }
  }

  if (isLoading) {
    return (
      <div className="flex items-center justify-center p-4">
        <Loader2 className="h-6 w-6 animate-spin text-muted-foreground" />
      </div>
    );
  }

  return (
    <div className="space-y-6">
      {error && (
        <Alert variant="destructive">
          <AlertDescription>{error}</AlertDescription>
        </Alert>
      )}

      {/* 连通检测语义说明：可达 ≠ 配置正确 */}
      <Alert>
        <Info className="h-4 w-4" />
        <AlertDescription>
          {t("streamCheck.connectivityNote", {
            defaultValue:
              "连通检测仅探测供应商地址是否可达，不发送真实模型请求。收到任意响应即视为“可达”——这不代表鉴权或模型配置一定正确。",
          })}
        </AlertDescription>
      </Alert>

      {/* 检查参数配置 */}
      <div className="space-y-4">
        <h4 className="text-sm font-medium text-muted-foreground">
          {t("streamCheck.checkParams")}
        </h4>
        <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
          <div className="space-y-2">
            <Label htmlFor="timeoutSecs">{t("streamCheck.timeout")}</Label>
            <Input
              id="timeoutSecs"
              type="number"
              min={2}
              max={60}
              value={config.timeoutSecs}
              onChange={(e) =>
                setConfig({ ...config, timeoutSecs: e.target.value })
              }
            />
          </div>

          <div className="space-y-2">
            <Label htmlFor="maxRetries">{t("streamCheck.maxRetries")}</Label>
            <Input
              id="maxRetries"
              type="number"
              min={0}
              max={5}
              value={config.maxRetries}
              onChange={(e) =>
                setConfig({ ...config, maxRetries: e.target.value })
              }
            />
          </div>

          <div className="space-y-2">
            <Label htmlFor="degradedThresholdMs">
              {t("streamCheck.degradedThreshold")}
            </Label>
            <Input
              id="degradedThresholdMs"
              type="number"
              min={1000}
              max={30000}
              step={1000}
              value={config.degradedThresholdMs}
              onChange={(e) =>
                setConfig({ ...config, degradedThresholdMs: e.target.value })
              }
            />
          </div>
        </div>
      </div>

      <div className="flex justify-end">
        <Button onClick={handleSave} disabled={isSaving}>
          {isSaving ? (
            <>
              <Loader2 className="mr-2 h-4 w-4 animate-spin" />
              {t("common.saving")}
            </>
          ) : (
            <>
              <Save className="mr-2 h-4 w-4" />
              {t("common.save")}
            </>
          )}
        </Button>
      </div>
    </div>
  );
}
