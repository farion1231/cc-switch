import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "@/lib/toast";
import { Switch } from "@/components/ui/switch";
import { Label } from "@/components/ui/label";
import { Input } from "@/components/ui/input";
import {
  settingsApi,
  type RectifierConfig,
  type OptimizerConfig,
} from "@/lib/api/settings";

const DEFAULT_IMAGE_CAP_MAX = 20;
const DEFAULT_IMAGE_CAP_DISCARD = 5;
const MAX_IMAGE_CAP_MAX = 1000;

/** 上限非法或未填时回落到默认值。 */
function normalizeImageCapMax(value: number): number {
  if (!Number.isFinite(value) || value < 1) return DEFAULT_IMAGE_CAP_MAX;
  return Math.min(Math.floor(value), MAX_IMAGE_CAP_MAX);
}

/** 单次丢弃不得超过上限，否则一次会丢掉远超所需的数量。 */
function normalizeImageCapDiscard(value: number, max: number): number {
  const limit = Math.max(1, normalizeImageCapMax(max));
  if (!Number.isFinite(value) || value < 1) {
    return Math.min(DEFAULT_IMAGE_CAP_DISCARD, limit);
  }
  return Math.min(Math.floor(value), limit);
}

function inRange(raw: string, max: number): boolean {
  const n = Number(raw);
  return raw.trim() !== "" && Number.isInteger(n) && n >= 1 && n <= max;
}

export function RectifierConfigPanel() {
  const { t } = useTranslation();
  const [config, setConfig] = useState<RectifierConfig>({
    enabled: true,
    requestThinkingSignature: true,
    requestThinkingBudget: true,
    requestMediaFallback: true,
    requestMediaHeuristic: true,
    imageCapEnabled: false,
    imageCapMax: DEFAULT_IMAGE_CAP_MAX,
    imageCapDiscard: DEFAULT_IMAGE_CAP_DISCARD,
  });
  // Numeric inputs keep a draft so empty or partial input never gets saved;
  // the value is committed on blur.
  const [imageCapMaxDraft, setImageCapMaxDraft] = useState(
    String(DEFAULT_IMAGE_CAP_MAX),
  );
  const [imageCapDiscardDraft, setImageCapDiscardDraft] = useState(
    String(DEFAULT_IMAGE_CAP_DISCARD),
  );
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

  // Sync the drafts when the stored config changes, whether from the initial
  // load or a rollback after a failed save.
  useEffect(() => {
    setImageCapMaxDraft(String(config.imageCapMax));
    setImageCapDiscardDraft(String(config.imageCapDiscard));
  }, [config.imageCapMax, config.imageCapDiscard]);

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

  /** 失焦提交：非法值回退到已存值，合法值取整后写入。 */
  const commitImageCapMax = () => {
    if (!inRange(imageCapMaxDraft, MAX_IMAGE_CAP_MAX)) {
      setImageCapMaxDraft(String(config.imageCapMax));
      return;
    }
    const value = Math.floor(Number(imageCapMaxDraft));
    setImageCapMaxDraft(String(value));
    if (value === config.imageCapMax) return;
    // Narrow the step when the cap drops below it, otherwise a single pass
    // would remove far more images than needed.
    // Commit both fields in one call so the second write cannot overwrite the
    // first with a stale snapshot.
    const discard = normalizeImageCapDiscard(config.imageCapDiscard, value);
    setImageCapDiscardDraft(String(discard));
    void handleChange({ imageCapMax: value, imageCapDiscard: discard });
  };

  const commitImageCapDiscard = () => {
    if (!inRange(imageCapDiscardDraft, config.imageCapMax)) {
      setImageCapDiscardDraft(String(config.imageCapDiscard));
      return;
    }
    const value = Math.floor(Number(imageCapDiscardDraft));
    setImageCapDiscardDraft(String(value));
    if (value === config.imageCapDiscard) return;
    void handleChange({ imageCapDiscard: value });
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
        </div>
        <Switch
          checked={config.enabled}
          onCheckedChange={(checked) => handleChange({ enabled: checked })}
        />
      </div>

      <div className="space-y-4">
        <h4 className="text-sm font-medium text-fg-2">
          {t("settings.advanced.rectifier.requestGroup")}
        </h4>
        <div className="flex items-center justify-between pl-4">
          <div className="space-y-0.5">
            <Label>{t("settings.advanced.rectifier.thinkingSignature")}</Label>
            <p className="text-xs text-fg-2">
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
            <p className="text-xs text-fg-2">
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
            <p className="text-xs text-fg-2">
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
            <p className="text-xs text-fg-2">
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
        <div className="flex items-center justify-between gap-4 pl-4">
          <div className="min-w-0 space-y-0.5">
            <Label>{t("settings.advanced.rectifier.imageCap")}</Label>
            <p className="text-xs text-fg-2">
              {t("settings.advanced.rectifier.imageCapDescription")}
            </p>
          </div>
          <Switch
            className="shrink-0"
            checked={config.imageCapEnabled}
            disabled={!config.enabled}
            onCheckedChange={(checked) => {
              // Write the normalized counts back when enabling: flipping the
              // switch alone would leave the feature on without usable limits.
              const max = normalizeImageCapMax(config.imageCapMax);
              handleChange({
                imageCapEnabled: checked,
                imageCapMax: max,
                imageCapDiscard: normalizeImageCapDiscard(
                  config.imageCapDiscard,
                  max,
                ),
              });
            }}
          />
        </div>
        {config.imageCapEnabled && config.enabled && (
          <div className="flex flex-wrap items-center gap-4 pl-8">
            <div className="flex items-center gap-2 text-xs text-fg-2">
              <label htmlFor="rectifier-image-cap-max">
                {t("settings.advanced.rectifier.imageCapMax")}
              </label>
              <Input
                id="rectifier-image-cap-max"
                type="number"
                min={1}
                max={MAX_IMAGE_CAP_MAX}
                step={1}
                value={imageCapMaxDraft}
                aria-invalid={!inRange(imageCapMaxDraft, MAX_IMAGE_CAP_MAX)}
                onChange={(event) => setImageCapMaxDraft(event.target.value)}
                onBlur={commitImageCapMax}
                className="w-20"
              />
            </div>
            <div className="flex items-center gap-2 text-xs text-fg-2">
              <label htmlFor="rectifier-image-cap-discard">
                {t("settings.advanced.rectifier.imageCapDiscard")}
              </label>
              <Input
                id="rectifier-image-cap-discard"
                type="number"
                min={1}
                max={config.imageCapMax}
                step={1}
                value={imageCapDiscardDraft}
                aria-invalid={
                  !inRange(imageCapDiscardDraft, config.imageCapMax)
                }
                onChange={(event) =>
                  setImageCapDiscardDraft(event.target.value)
                }
                onBlur={commitImageCapDiscard}
                className="w-20"
              />
            </div>
          </div>
        )}
      </div>

      <div className="border-t pt-6 mt-6">
        <div className="space-y-1 mb-4">
          <h3 className="text-sm font-medium">
            {t("settings.advanced.optimizer.title")}
          </h3>
          <p className="text-xs text-fg-2">
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
                <p className="text-xs text-fg-2">
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
                <p className="text-xs text-fg-2">
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
