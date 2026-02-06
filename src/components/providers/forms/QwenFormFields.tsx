import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { useTranslation } from "react-i18next";

interface QwenFormFieldsProps {
  settingsConfig: Record<string, any>;
  onChange: (config: Record<string, any>) => void;
}

export function QwenFormFields({ settingsConfig, onChange }: QwenFormFieldsProps) {
  const { t } = useTranslation();
  const env = settingsConfig.env || {};

  const handleEnvChange = (key: string, value: string) => {
    onChange({
      ...settingsConfig,
      env: {
        ...env,
        [key]: value,
      },
    });
  };

  return (
    <div className="space-y-4">
      <div className="space-y-2">
        <Label htmlFor="qwen-api-key">
          {t("provider.openaiApiKey", "OpenAI API Key")}
        </Label>
        <Input
          id="qwen-api-key"
          type="password"
          value={env.OPENAI_API_KEY || ""}
          onChange={(e) => handleEnvChange("OPENAI_API_KEY", e.target.value)}
          placeholder="sk-..."
        />
        <p className="text-xs text-muted-foreground">
          {t("provider.qwenApiKeyHint", "Your Qwen/DashScope API key")}
        </p>
      </div>

      <div className="space-y-2">
        <Label htmlFor="qwen-base-url">
          {t("provider.baseUrl", "Base URL")}
        </Label>
        <Input
          id="qwen-base-url"
          value={env.OPENAI_BASE_URL || ""}
          onChange={(e) => handleEnvChange("OPENAI_BASE_URL", e.target.value)}
          placeholder="https://dashscope.aliyuncs.com/compatible-mode/v1"
        />
      </div>

      <div className="space-y-2">
        <Label htmlFor="qwen-model">
          {t("provider.model", "Model")}
        </Label>
        <Input
          id="qwen-model"
          value={env.OPENAI_MODEL || ""}
          onChange={(e) => handleEnvChange("OPENAI_MODEL", e.target.value)}
          placeholder="qwen3-coder-plus"
        />
      </div>
    </div>
  );
}
