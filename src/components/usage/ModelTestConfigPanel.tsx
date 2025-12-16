import { useState, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Save, Loader2 } from "lucide-react";
import { toast } from "sonner";
import {
  getStreamCheckConfig,
  saveStreamCheckConfig,
  type StreamCheckConfig,
} from "@/lib/api/model-test";

export function ModelTestConfigPanel() {
  const { t } = useTranslation();
  const [isLoading, setIsLoading] = useState(true);
  const [isSaving, setIsSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [config, setConfig] = useState<StreamCheckConfig>({
    timeoutSecs: 45,
    maxRetries: 2,
    degradedThresholdMs: 6000,
    claudeModel: "claude-haiku-4-5-20251001",
    codexModel: "gpt-5.1-codex@low",
    geminiModel: "gemini-3-pro-preview",
  });

  useEffect(() => {
    loadConfig();
  }, []);

  async function loadConfig() {
    try {
      setIsLoading(true);
      setError(null);
      const data = await getStreamCheckConfig();
      setConfig(data);
    } catch (e) {
      setError(String(e));
    } finally {
      setIsLoading(false);
    }
  }

  async function handleSave() {
    try {
      setIsSaving(true);
      await saveStreamCheckConfig(config);
      toast.success(t("streamCheck.configSaved", "健康检查配置已保存"), {
        closeButton: true,
      });
    } catch (e) {
      toast.error(
        t("streamCheck.configSaveFailed", "保存失败") + ": " + String(e),
      );
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

      {/* 测试模型配置 */}
      <div className="space-y-4">
        <h4 className="text-sm font-medium text-muted-foreground">
          {t("streamCheck.testModels", "测试模型")}
        </h4>
        <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
          <div className="space-y-2">
            <Label htmlFor="claudeModel">
              {t("streamCheck.claudeModel", "Claude 模型")}
            </Label>
            <Input
              id="claudeModel"
              value={config.claudeModel}
              onChange={(e) =>
                setConfig({ ...config, claudeModel: e.target.value })
              }
              placeholder="claude-3-5-haiku-latest"
            />
          </div>

          <div className="space-y-2">
            <Label htmlFor="codexModel">
              {t("streamCheck.codexModel", "Codex 模型")}
            </Label>
            <Input
              id="codexModel"
              value={config.codexModel}
              onChange={(e) =>
                setConfig({ ...config, codexModel: e.target.value })
              }
              placeholder="gpt-4o-mini"
            />
          </div>

          <div className="space-y-2">
            <Label htmlFor="geminiModel">
              {t("streamCheck.geminiModel", "Gemini 模型")}
            </Label>
            <Input
              id="geminiModel"
              value={config.geminiModel}
              onChange={(e) =>
                setConfig({ ...config, geminiModel: e.target.value })
              }
              placeholder="gemini-1.5-flash"
            />
          </div>
        </div>
      </div>

      {/* 检查参数配置 */}
      <div className="space-y-4">
        <h4 className="text-sm font-medium text-muted-foreground">
          {t("streamCheck.checkParams", "检查参数")}
        </h4>
        <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
          <div className="space-y-2">
            <Label htmlFor="timeoutSecs">
              {t("streamCheck.timeout", "超时时间（秒）")}
            </Label>
            <Input
              id="timeoutSecs"
              type="number"
              min={10}
              max={120}
              value={config.timeoutSecs}
              onChange={(e) =>
                setConfig({
                  ...config,
                  timeoutSecs: parseInt(e.target.value) || 45,
                })
              }
            />
          </div>

          <div className="space-y-2">
            <Label htmlFor="maxRetries">
              {t("streamCheck.maxRetries", "最大重试次数")}
            </Label>
            <Input
              id="maxRetries"
              type="number"
              min={0}
              max={5}
              value={config.maxRetries}
              onChange={(e) =>
                setConfig({
                  ...config,
                  maxRetries: parseInt(e.target.value) || 2,
                })
              }
            />
          </div>

          <div className="space-y-2">
            <Label htmlFor="degradedThresholdMs">
              {t("streamCheck.degradedThreshold", "降级阈值（毫秒）")}
            </Label>
            <Input
              id="degradedThresholdMs"
              type="number"
              min={1000}
              max={30000}
              step={1000}
              value={config.degradedThresholdMs}
              onChange={(e) =>
                setConfig({
                  ...config,
                  degradedThresholdMs: parseInt(e.target.value) || 6000,
                })
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
    </div>
  );
}
