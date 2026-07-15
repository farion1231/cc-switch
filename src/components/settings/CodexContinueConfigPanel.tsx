import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import { settingsApi, type CodexContinueConfig } from "@/lib/api/settings";

const DEFAULT_CODEX_CONTINUE_CONFIG: CodexContinueConfig = {
  enabled: true,
  maxContinuations: 8,
  step: 518,
  marker:
    "We need continue thinking. Do not summarize; continue from the previous reasoning state.",
};

export function CodexContinueConfigPanel() {
  const { t } = useTranslation();
  const [config, setConfig] = useState<CodexContinueConfig>(
    DEFAULT_CODEX_CONTINUE_CONFIG,
  );
  const [draft, setDraft] = useState<CodexContinueConfig>(
    DEFAULT_CODEX_CONTINUE_CONFIG,
  );
  const [isLoading, setIsLoading] = useState(true);

  useEffect(() => {
    settingsApi
      .getCodexContinueConfig()
      .then((loaded) => {
        setConfig(loaded);
        setDraft(loaded);
      })
      .catch((e) => console.error("Failed to load CodexCont config:", e))
      .finally(() => setIsLoading(false));
  }, []);

  const handleChange = async (updates: Partial<CodexContinueConfig>) => {
    const newConfig = { ...config, ...updates };
    setConfig(newConfig);
    setDraft(newConfig);
    try {
      await settingsApi.setCodexContinueConfig(newConfig);
      toast.success(
        newConfig.enabled
          ? t("settings.advanced.codexContinue.enabledToast", {
              defaultValue: "CodexCont 自动续写已启用",
            })
          : t("settings.advanced.codexContinue.disabledToast", {
              defaultValue: "CodexCont 自动续写已关闭",
            }),
        { closeButton: true },
      );
    } catch (e) {
      console.error("Failed to save CodexCont config:", e);
      toast.error(String(e));
      setConfig(config);
      setDraft(config);
    }
  };

  const handleSaveAdvanced = async () => {
    const maxContinuations = Math.max(0, Math.floor(draft.maxContinuations));
    const step = Math.max(3, Math.floor(draft.step));
    const marker = draft.marker.trim() || DEFAULT_CODEX_CONTINUE_CONFIG.marker;
    await handleChange({ maxContinuations, step, marker });
  };

  if (isLoading) return null;

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <div className="space-y-0.5">
          <Label>
            {t("settings.advanced.codexContinue.enabled", {
              defaultValue: "启用自动续写",
            })}
          </Label>
          <p className="text-xs text-muted-foreground">
            {t("settings.advanced.codexContinue.enabledDescription", {
              defaultValue:
                "关闭后 Codex 请求仍通过 CC Switch 路由，但不再做 CodexCont 折叠续写。",
            })}
          </p>
        </div>
        <Switch
          checked={config.enabled}
          onCheckedChange={(checked) => handleChange({ enabled: checked })}
        />
      </div>

      <div className="grid gap-4 md:grid-cols-2">
        <div className="space-y-2">
          <Label htmlFor="codex-continue-max">
            {t("settings.advanced.codexContinue.maxContinuations", {
              defaultValue: "最大续写轮数",
            })}
          </Label>
          <Input
            id="codex-continue-max"
            type="number"
            min={0}
            max={32}
            value={draft.maxContinuations}
            onChange={(event) =>
              setDraft((prev) => ({
                ...prev,
                maxContinuations: Number(event.target.value),
              }))
            }
          />
        </div>
        <div className="space-y-2">
          <Label htmlFor="codex-continue-step">
            {t("settings.advanced.codexContinue.step", {
              defaultValue: "截断步长",
            })}
          </Label>
          <Input
            id="codex-continue-step"
            type="number"
            min={3}
            value={draft.step}
            onChange={(event) =>
              setDraft((prev) => ({
                ...prev,
                step: Number(event.target.value),
              }))
            }
          />
        </div>
      </div>

      <div className="space-y-2">
        <Label htmlFor="codex-continue-marker">
          {t("settings.advanced.codexContinue.marker", {
            defaultValue: "续写提示",
          })}
        </Label>
        <Textarea
          id="codex-continue-marker"
          value={draft.marker}
          onChange={(event) =>
            setDraft((prev) => ({ ...prev, marker: event.target.value }))
          }
          rows={3}
        />
        <p className="text-xs text-muted-foreground">
          {t("settings.advanced.codexContinue.markerDescription", {
            defaultValue:
              "用于触发下一轮 reasoning continuation；环境变量仍可临时覆盖这些参数。",
          })}
        </p>
      </div>

      <div className="flex justify-end">
        <Button size="sm" onClick={() => void handleSaveAdvanced()}>
          {t("common.save", { defaultValue: "保存" })}
        </Button>
      </div>
    </div>
  );
}
