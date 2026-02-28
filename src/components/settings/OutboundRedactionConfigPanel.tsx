import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Plus, Trash2 } from "lucide-react";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  settingsApi,
  type CustomRedactionRule,
  type OutboundRedactionConfig,
} from "@/lib/api/settings";

const DEFAULT_CONFIG: OutboundRedactionConfig = {
  enabled: false,
  onError: "warn_and_bypass",
  customRules: [],
};
const PATTERN_SAVE_DEBOUNCE_MS = 300;

function normalizeConfig(
  config: OutboundRedactionConfig,
): OutboundRedactionConfig {
  return {
    ...config,
    customRules: (config.customRules ?? []).map((rule) => ({
      ...rule,
      matchMethod: rule.matchMethod ?? "regex",
    })),
  };
}

export function OutboundRedactionConfigPanel() {
  const { t } = useTranslation();
  const [config, setConfig] = useState<OutboundRedactionConfig>(DEFAULT_CONFIG);
  const [isLoading, setIsLoading] = useState(true);
  const pendingSaveSeqRef = useRef(0);
  const committedSaveSeqRef = useRef(0);
  const lastCommittedConfigRef = useRef<OutboundRedactionConfig>(DEFAULT_CONFIG);
  const saveDebounceTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const queuedSaveConfigRef = useRef<OutboundRedactionConfig | null>(null);

  useEffect(() => {
    settingsApi
      .getOutboundRedactionConfig()
      .then((remote) => {
        const normalized = normalizeConfig(remote);
        setConfig(normalized);
        lastCommittedConfigRef.current = normalized;
      })
      .catch((e) => {
        console.error("Failed to load outbound redaction config:", e);
        toast.error(String(e));
      })
      .finally(() => setIsLoading(false));
  }, []);

  const persistConfig = async (normalized: OutboundRedactionConfig) => {
    const seq = ++pendingSaveSeqRef.current;
    try {
      await settingsApi.setOutboundRedactionConfig(normalized);
      if (seq > committedSaveSeqRef.current) {
        committedSaveSeqRef.current = seq;
        lastCommittedConfigRef.current = normalized;
      }
    } catch (e) {
      console.error("Failed to save outbound redaction config:", e);
      toast.error(String(e));
      // Only rollback when the latest write fails to avoid older request races
      if (seq === pendingSaveSeqRef.current) {
        setConfig(lastCommittedConfigRef.current);
      }
    }
  };

  const flushQueuedSave = () => {
    if (saveDebounceTimerRef.current) {
      clearTimeout(saveDebounceTimerRef.current);
      saveDebounceTimerRef.current = null;
    }
    const queued = queuedSaveConfigRef.current;
    if (!queued) return;
    queuedSaveConfigRef.current = null;
    void persistConfig(queued);
  };

  const schedulePersist = (
    normalized: OutboundRedactionConfig,
    debounceMs: number,
  ) => {
    if (saveDebounceTimerRef.current) {
      clearTimeout(saveDebounceTimerRef.current);
      saveDebounceTimerRef.current = null;
    }

    if (debounceMs <= 0) {
      queuedSaveConfigRef.current = null;
      void persistConfig(normalized);
      return;
    }

    queuedSaveConfigRef.current = normalized;
    saveDebounceTimerRef.current = setTimeout(() => {
      saveDebounceTimerRef.current = null;
      const queued = queuedSaveConfigRef.current;
      queuedSaveConfigRef.current = null;
      if (!queued) return;
      void persistConfig(queued);
    }, debounceMs);
  };

  const applyConfig = (
    updater: (previous: OutboundRedactionConfig) => OutboundRedactionConfig,
    options?: { debounceMs?: number },
  ) => {
    setConfig((previous) => {
      const next = normalizeConfig(updater(previous));
      schedulePersist(next, options?.debounceMs ?? 0);
      return next;
    });
  };

  const updateRule = (
    index: number,
    updates: Partial<CustomRedactionRule>,
    options?: { debounceMs?: number },
  ) => {
    applyConfig((previous) => {
      const nextRules = [...previous.customRules];
      nextRules[index] = { ...nextRules[index], ...updates };
      return { ...previous, customRules: nextRules };
    }, options);
  };

  const addRule = () => {
    const newRule: CustomRedactionRule = {
      enabled: false,
      matchMethod: "regex",
      pattern: "",
    };
    applyConfig((previous) => ({
      ...previous,
      customRules: [...previous.customRules, newRule],
    }));
  };

  const removeRule = (index: number) => {
    applyConfig((previous) => ({
      ...previous,
      customRules: previous.customRules.filter((_, i) => i !== index),
    }));
  };

  useEffect(() => {
    return () => {
      if (saveDebounceTimerRef.current) {
        clearTimeout(saveDebounceTimerRef.current);
      }
    };
  }, []);

  if (isLoading) return null;

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div className="space-y-0.5">
          <Label>{t("settings.advanced.outboundRedaction.enabled")}</Label>
          <p className="text-xs text-muted-foreground">
            {t("settings.advanced.outboundRedaction.enabledDescription")}
          </p>
        </div>
        <Switch
          checked={config.enabled}
          onCheckedChange={(checked) =>
            applyConfig((previous) => ({ ...previous, enabled: checked }))
          }
        />
      </div>

      <div className="space-y-1.5">
        <Label>
          {t("settings.advanced.outboundRedaction.errorStrategy")}
        </Label>
        <Select
          value={config.onError}
          disabled={!config.enabled}
          onValueChange={(value) =>
            applyConfig((previous) => ({
              ...previous,
              onError: value as OutboundRedactionConfig["onError"],
            }))
          }
        >
          <SelectTrigger>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="warn_and_bypass">
              {t(
                "settings.advanced.outboundRedaction.errorStrategies.warn_and_bypass",
              )}
            </SelectItem>
            <SelectItem value="block_request">
              {t(
                "settings.advanced.outboundRedaction.errorStrategies.block_request",
              )}
            </SelectItem>
          </SelectContent>
        </Select>
      </div>

      <div className="space-y-4">
        <div className="flex items-center justify-between">
          <h4 className="text-sm font-medium text-muted-foreground">
            {t("settings.advanced.outboundRedaction.customRules")}
          </h4>
          <Button
            size="sm"
            variant="outline"
            disabled={!config.enabled}
            onClick={addRule}
          >
            <Plus className="h-3.5 w-3.5" />
            {t("settings.advanced.outboundRedaction.addRule")}
          </Button>
        </div>

        <p className="text-xs text-muted-foreground">
          {t("settings.advanced.outboundRedaction.customRulesDescription")}
        </p>

        <div className="space-y-3">
          {config.customRules.map((rule, index) => (
            <div
              key={`rule-${index}`}
              className="rounded-lg border border-border/60 p-3 space-y-3"
            >
              <div className="flex items-center justify-between gap-3">
                <Label className="text-xs text-muted-foreground">
                  #{index + 1}
                </Label>
                <div className="flex items-center gap-2">
                  <Select
                    value={rule.matchMethod}
                    disabled={!config.enabled}
                    onValueChange={(value) =>
                      updateRule(index, {
                        matchMethod:
                          value as CustomRedactionRule["matchMethod"],
                      })
                    }
                  >
                    <SelectTrigger className="w-[130px]">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="regex">
                        {t(
                          "settings.advanced.outboundRedaction.matchMethods.regex",
                        )}
                      </SelectItem>
                      <SelectItem value="string_match">
                        {t(
                          "settings.advanced.outboundRedaction.matchMethods.string_match",
                        )}
                      </SelectItem>
                    </SelectContent>
                  </Select>
                  <Switch
                    checked={rule.enabled}
                    disabled={!config.enabled}
                    onCheckedChange={(checked) =>
                      updateRule(index, { enabled: checked })
                    }
                  />
                  <Button
                    size="icon"
                    variant="ghost"
                    disabled={!config.enabled}
                    onClick={() => removeRule(index)}
                  >
                    <Trash2 className="h-4 w-4 text-red-500" />
                  </Button>
                </div>
              </div>
              <Input
                value={rule.pattern}
                disabled={!config.enabled}
                placeholder={t(
                  "settings.advanced.outboundRedaction.patternPlaceholder",
                )}
                onChange={(e) => {
                  const value = e.currentTarget.value;
                  updateRule(
                    index,
                    { pattern: value },
                    { debounceMs: PATTERN_SAVE_DEBOUNCE_MS },
                  );
                }}
                onBlur={flushQueuedSave}
              />
            </div>
          ))}
          {config.customRules.length === 0 && (
            <div className="rounded-lg border border-dashed border-border/70 p-4 text-xs text-muted-foreground">
              {t("settings.advanced.outboundRedaction.noCustomRules")}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
