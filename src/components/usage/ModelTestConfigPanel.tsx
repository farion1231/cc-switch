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
import { Alert, AlertDescription } from "@/components/ui/alert";
import { ChevronDown, ChevronRight, Save, Loader2 } from "lucide-react";
import { toast } from "sonner";
import {
  getModelTestConfig,
  saveModelTestConfig,
  type ModelTestConfig,
} from "@/lib/api/model-test";

export function ModelTestConfigPanel() {
  const { t } = useTranslation();
  const [isExpanded, setIsExpanded] = useState(false);
  const [isLoading, setIsLoading] = useState(true);
  const [isSaving, setIsSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [config, setConfig] = useState<ModelTestConfig>({
    claudeModel: "claude-haiku-4-5-20251001",
    codexModel: "gpt-5.1-low",
    geminiModel: "gemini-3-pro-low",
    testPrompt: "ping",
    timeoutSecs: 15,
  });

  useEffect(() => {
    loadConfig();
  }, []);

  async function loadConfig() {
    try {
      setIsLoading(true);
      setError(null);
      const data = await getModelTestConfig();
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
      await saveModelTestConfig(config);
      toast.success(t("modelTest.configSaved", "模型测试配置已保存"));
    } catch (e) {
      toast.error(
        t("modelTest.configSaveFailed", "保存失败") + ": " + String(e),
      );
    } finally {
      setIsSaving(false);
    }
  }

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
              {t("modelTest.configTitle", "模型测试配置")}
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
          <div>
            <CardTitle className="text-base">
              {t("modelTest.configTitle", "模型测试配置")}
            </CardTitle>
            {!isExpanded && (
              <CardDescription className="mt-1">
                {t(
                  "modelTest.configDesc",
                  "配置模型测试使用的默认模型和提示词",
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
              <AlertDescription>{error}</AlertDescription>
            </Alert>
          )}

          <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
            <div className="space-y-2">
              <Label htmlFor="claudeModel">
                {t("modelTest.claudeModel", "Claude 测试模型")}
              </Label>
              <Input
                id="claudeModel"
                value={config.claudeModel}
                onChange={(e) =>
                  setConfig({ ...config, claudeModel: e.target.value })
                }
                placeholder="claude-haiku-4-5-20251001"
              />
            </div>

            <div className="space-y-2">
              <Label htmlFor="codexModel">
                {t("modelTest.codexModel", "Codex 测试模型")}
              </Label>
              <Input
                id="codexModel"
                value={config.codexModel}
                onChange={(e) =>
                  setConfig({ ...config, codexModel: e.target.value })
                }
                placeholder="gpt-5.1-low"
              />
            </div>

            <div className="space-y-2">
              <Label htmlFor="geminiModel">
                {t("modelTest.geminiModel", "Gemini 测试模型")}
              </Label>
              <Input
                id="geminiModel"
                value={config.geminiModel}
                onChange={(e) =>
                  setConfig({ ...config, geminiModel: e.target.value })
                }
                placeholder="gemini-3-pro-low"
              />
            </div>
          </div>

          <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
            <div className="space-y-2">
              <Label htmlFor="testPrompt">
                {t("modelTest.testPrompt", "测试提示词")}
              </Label>
              <Input
                id="testPrompt"
                value={config.testPrompt}
                onChange={(e) =>
                  setConfig({ ...config, testPrompt: e.target.value })
                }
                placeholder="ping"
              />
              <p className="text-xs text-muted-foreground">
                {t(
                  "modelTest.testPromptHint",
                  "发送给模型的测试消息，建议使用简短内容以减少 token 消耗",
                )}
              </p>
            </div>

            <div className="space-y-2">
              <Label htmlFor="timeoutSecs">
                {t("modelTest.timeout", "超时时间（秒）")}
              </Label>
              <Input
                id="timeoutSecs"
                type="number"
                min={5}
                max={60}
                value={config.timeoutSecs}
                onChange={(e) =>
                  setConfig({
                    ...config,
                    timeoutSecs: parseInt(e.target.value) || 15,
                  })
                }
              />
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
        </CardContent>
      )}
    </Card>
  );
}
