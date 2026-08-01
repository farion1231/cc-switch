import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Bot, Loader2, RotateCcw, Save } from "lucide-react";
import { toast } from "sonner";
import { codexSubagentsApi } from "@/lib/api/codexSubagents";
import { extractErrorMessage } from "@/utils/errorUtils";
import type { CodexSubagentSettingsView } from "@/types";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";

interface AgentsPanelProps {
  onOpenChange: (open: boolean) => void;
}

const UNSET_SENTINEL = "__unset__";
const REASONING_EFFORTS = ["low", "medium", "high", "xhigh", "max", "ultra"];

export function AgentsPanel({}: AgentsPanelProps) {
  const { t } = useTranslation();
  const [settings, setSettings] = useState<CodexSubagentSettingsView | null>(
    null,
  );
  const [model, setModel] = useState("");
  const [reasoningEffort, setReasoningEffort] = useState("");
  const [isLoading, setIsLoading] = useState(true);
  const [isSaving, setIsSaving] = useState(false);

  useEffect(() => {
    let cancelled = false;

    void codexSubagentsApi
      .getSettings()
      .then((loaded) => {
        if (cancelled) return;
        setSettings(loaded);
        setModel(loaded.model ?? "");
        setReasoningEffort(loaded.reasoningEffort ?? "");
      })
      .catch((error) => {
        if (!cancelled) {
          toast.error(t("agents.loadFailed"), {
            description: extractErrorMessage(error) || undefined,
          });
        }
      })
      .finally(() => {
        if (!cancelled) setIsLoading(false);
      });

    return () => {
      cancelled = true;
    };
  }, [t]);

  const handleSave = async () => {
    setIsSaving(true);
    try {
      const saved = await codexSubagentsApi.saveSettings(
        model.trim() || null,
        reasoningEffort || null,
      );
      setSettings(saved);
      setModel(saved.model ?? "");
      setReasoningEffort(saved.reasoningEffort ?? "");
      toast.success(t("agents.saveSuccess"));
    } catch (error) {
      toast.error(t("agents.saveFailed"), {
        description: extractErrorMessage(error) || undefined,
      });
    } finally {
      setIsSaving(false);
    }
  };

  const handleReset = () => {
    setModel(settings?.model ?? "");
    setReasoningEffort(settings?.reasoningEffort ?? "");
  };

  if (isLoading) {
    return (
      <div className="px-6 flex flex-1 items-center justify-center min-h-[200px]">
        <Loader2 className="h-5 w-5 animate-spin text-muted-foreground" />
      </div>
    );
  }

  return (
    <div className="px-6 pt-4 pb-8 flex flex-col flex-1 min-h-0 overflow-y-auto">
      <div className="rounded-xl border border-border bg-card p-5 max-w-3xl w-full">
        <div className="flex items-start gap-3 mb-6">
          <div className="rounded-lg bg-blue-500/10 p-2 text-blue-500">
            <Bot className="h-5 w-5" />
          </div>
          <div>
            <h2 className="text-base font-semibold">
              {t("agents.codexTitle")}
            </h2>
            <p className="text-sm text-muted-foreground mt-1">
              {t("agents.codexDescription")}
            </p>
          </div>
        </div>

        <div className="grid grid-cols-1 md:grid-cols-2 gap-5">
          <div>
            <Label htmlFor="codex-subagent-model" className="mb-1.5 block">
              {t("agents.modelLabel")}
            </Label>
            <Input
              id="codex-subagent-model"
              list="codex-subagent-model-options"
              value={model}
              onChange={(event) => setModel(event.target.value)}
              placeholder={t("agents.modelPlaceholder")}
              className="font-mono text-xs"
            />
            <datalist id="codex-subagent-model-options">
              {settings?.availableModels.map((availableModel) => (
                <option key={availableModel} value={availableModel} />
              ))}
            </datalist>
            <p className="text-xs text-muted-foreground mt-1.5">
              {t("agents.modelHint")}
            </p>
          </div>

          <div>
            <Label className="mb-1.5 block">{t("agents.reasoningLabel")}</Label>
            <Select
              value={reasoningEffort || UNSET_SENTINEL}
              onValueChange={(value) =>
                setReasoningEffort(value === UNSET_SENTINEL ? "" : value)
              }
            >
              <SelectTrigger className="font-mono text-xs">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value={UNSET_SENTINEL}>
                  {t("agents.notSet")}
                </SelectItem>
                {REASONING_EFFORTS.map((effort) => (
                  <SelectItem key={effort} value={effort}>
                    {t(`agents.reasoning.${effort}`)}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <p className="text-xs text-muted-foreground mt-1.5">
              {t("agents.reasoningHint")}
            </p>
          </div>
        </div>

        <div className="mt-6 rounded-md bg-muted/40 px-3 py-2 text-xs text-muted-foreground">
          <span>{t("agents.configPath")}: </span>
          <span className="font-mono break-all">{settings?.configPath}</span>
        </div>

        <div className="flex items-center justify-end gap-2 mt-6">
          <Button
            variant="ghost"
            size="icon"
            onClick={handleReset}
            disabled={isSaving}
            title={t("agents.reset")}
            aria-label={t("agents.reset")}
          >
            <RotateCcw className="h-4 w-4" />
          </Button>
          <Button onClick={() => void handleSave()} disabled={isSaving}>
            {isSaving ? (
              <Loader2 className="h-4 w-4 animate-spin" />
            ) : (
              <Save className="h-4 w-4" />
            )}
            {t("common.save")}
          </Button>
        </div>
      </div>
    </div>
  );
}
