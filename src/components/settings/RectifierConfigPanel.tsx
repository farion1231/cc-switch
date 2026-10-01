import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Switch } from "@/components/ui/switch";
import { Label } from "@/components/ui/label";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import {
  settingsApi,
  type RectifierConfig,
  type OptimizerConfig,
} from "@/lib/api/settings";

/** 图片上限快捷档位。0 = 关闭（不设上限）。 */
const MEDIA_MAX_IMAGES_PRESETS = [0, 10, 20, 50, 70] as const;

export function RectifierConfigPanel() {
  const { t } = useTranslation();
  const [config, setConfig] = useState<RectifierConfig>({
    enabled: true,
    requestThinkingSignature: true,
    requestThinkingBudget: true,
    requestMediaFallback: true,
    requestMediaHeuristic: true,
    requestMediaMaxImages: 0,
  });
  const [optimizerConfig, setOptimizerConfig] = useState<OptimizerConfig>({
    enabled: false,
    thinkingOptimizer: true,
    cacheInjection: true,
  });
  const [isLoading, setIsLoading] = useState(true);

  useEffect(() => {
    settingsApi
      .getRectifierConfig()
      .then(setConfig)
      .catch((e) => console.error("Failed to load rectifier config:", e))
      .finally(() => setIsLoading(false));
    settingsApi
      .getOptimizerConfig()
      .then(setOptimizerConfig)
      .catch((e) => console.error("Failed to load optimizer config:", e));
  }, []);

  const handleChange = async (updates: Partial<RectifierConfig>) => {
    const newConfig = { ...config, ...updates };
    setConfig(newConfig);
    try {
      await settingsApi.setRectifierConfig(newConfig);
    } catch (e) {
      console.error("Failed to save rectifier config:", e);
      toast.error(String(e));
      setConfig(config);
    }
  };

  const handleOptimizerChange = async (updates: Partial<OptimizerConfig>) => {
    const newConfig = { ...optimizerConfig, ...updates };
    setOptimizerConfig(newConfig);
    try {
      await settingsApi.setOptimizerConfig(newConfig);
    } catch (e) {
      console.error("Failed to save optimizer config:", e);
      toast.error(String(e));
      setOptimizerConfig(optimizerConfig);
    }
  };

  if (isLoading) return null;

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div className="space-y-0.5">
          <Label>{t("settings.advanced.rectifier.enabled")}</Label>
          <p className="text-xs text-muted-foreground">
            {t("settings.advanced.rectifier.enabledDescription")}
          </p>
        </div>
        <Switch
          checked={config.enabled}
          onCheckedChange={(checked) => handleChange({ enabled: checked })}
        />
      </div>

      <div className="space-y-4">
        <h4 className="text-sm font-medium text-muted-foreground">
          {t("settings.advanced.rectifier.requestGroup")}
        </h4>
        <div className="flex items-center justify-between pl-4">
          <div className="space-y-0.5">
            <Label>{t("settings.advanced.rectifier.thinkingSignature")}</Label>
            <p className="text-xs text-muted-foreground">
              {t("settings.advanced.rectifier.thinkingSignatureDescription")}
            </p>
          </div>
          <Switch
            checked={config.requestThinkingSignature}
            disabled={!config.enabled}
            onCheckedChange={(checked) =>
              handleChange({ requestThinkingSignature: checked })
            }
          />
        </div>
        <div className="flex items-center justify-between pl-4">
          <div className="space-y-0.5">
            <Label>{t("settings.advanced.rectifier.thinkingBudget")}</Label>
            <p className="text-xs text-muted-foreground">
              {t("settings.advanced.rectifier.thinkingBudgetDescription")}
            </p>
          </div>
          <Switch
            checked={config.requestThinkingBudget}
            disabled={!config.enabled}
            onCheckedChange={(checked) =>
              handleChange({ requestThinkingBudget: checked })
            }
          />
        </div>
        <div className="flex items-center justify-between pl-4">
          <div className="space-y-0.5">
            <Label>{t("settings.advanced.rectifier.mediaFallback")}</Label>
            <p className="text-xs text-muted-foreground">
              {t("settings.advanced.rectifier.mediaFallbackDescription")}
            </p>
          </div>
          <Switch
            checked={config.requestMediaFallback}
            disabled={!config.enabled}
            onCheckedChange={(checked) =>
              handleChange({ requestMediaFallback: checked })
            }
          />
        </div>
        <div className="flex items-center justify-between pl-8">
          <div className="space-y-0.5">
            <Label>{t("settings.advanced.rectifier.mediaHeuristic")}</Label>
            <p className="text-xs text-muted-foreground">
              {t("settings.advanced.rectifier.mediaHeuristicDescription")}
            </p>
          </div>
          <Switch
            checked={config.requestMediaHeuristic}
            disabled={!config.enabled || !config.requestMediaFallback}
            onCheckedChange={(checked) =>
              handleChange({ requestMediaHeuristic: checked })
            }
          />
        </div>
        <div className="space-y-2 pl-8">
          <Label htmlFor="rectifier-media-max-images">
            {t("settings.advanced.rectifier.mediaMaxImages")}
          </Label>
          <div className="flex items-center gap-3">
            <Input
              id="rectifier-media-max-images"
              type="number"
              min={0}
              step={1}
              className="h-9 w-32"
              disabled={!config.enabled}
              value={String(config.requestMediaMaxImages)}
              onChange={(event) => {
                const parsed = Number.parseInt(event.target.value, 10);
                handleChange({
                  requestMediaMaxImages:
                    Number.isFinite(parsed) && parsed > 0 ? parsed : 0,
                });
              }}
            />
            <div className="flex flex-wrap items-center gap-2">
              {MEDIA_MAX_IMAGES_PRESETS.map((preset) => (
                <Button
                  key={preset}
                  type="button"
                  variant={
                    config.requestMediaMaxImages === preset
                      ? "default"
                      : "outline"
                  }
                  size="sm"
                  className="h-7 px-2 text-xs"
                  disabled={!config.enabled}
                  onClick={() =>
                    handleChange({ requestMediaMaxImages: preset })
                  }
                >
                  {preset}
                </Button>
              ))}
            </div>
          </div>
          <p className="text-xs text-muted-foreground">
            {t("settings.advanced.rectifier.mediaMaxImagesDescription")}
          </p>
        </div>
      </div>

      <div className="border-t pt-6 mt-6">
        <div className="space-y-1 mb-4">
          <h3 className="text-sm font-medium">
            {t("settings.advanced.optimizer.title")}
          </h3>
          <p className="text-xs text-muted-foreground">
            {t("settings.advanced.optimizer.description")}
          </p>
        </div>

        <div className="space-y-4">
          <div className="flex items-center justify-between">
            <div className="space-y-0.5">
              <Label>{t("settings.advanced.optimizer.enabled")}</Label>
            </div>
            <Switch
              checked={optimizerConfig.enabled}
              onCheckedChange={(checked) =>
                handleOptimizerChange({ enabled: checked })
              }
            />
          </div>

          <div className="space-y-4 pl-4">
            <div className="flex items-center justify-between">
              <div className="space-y-0.5">
                <Label>
                  {t("settings.advanced.optimizer.thinkingOptimizer")}
                </Label>
                <p className="text-xs text-muted-foreground">
                  {t(
                    "settings.advanced.optimizer.thinkingOptimizerDescription",
                  )}
                </p>
              </div>
              <Switch
                checked={optimizerConfig.thinkingOptimizer}
                disabled={!optimizerConfig.enabled}
                onCheckedChange={(checked) =>
                  handleOptimizerChange({ thinkingOptimizer: checked })
                }
              />
            </div>

            <div className="flex items-center justify-between">
              <div className="space-y-0.5">
                <Label>{t("settings.advanced.optimizer.cacheInjection")}</Label>
                <p className="text-xs text-muted-foreground">
                  {t("settings.advanced.optimizer.cacheInjectionDescription")}
                </p>
              </div>
              <Switch
                checked={optimizerConfig.cacheInjection}
                disabled={!optimizerConfig.enabled}
                onCheckedChange={(checked) =>
                  handleOptimizerChange({ cacheInjection: checked })
                }
              />
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
