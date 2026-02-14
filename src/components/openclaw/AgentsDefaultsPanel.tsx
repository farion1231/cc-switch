import React, { useState, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { Save } from "lucide-react";
import { toast } from "sonner";
import {
  useOpenClawAgentsDefaults,
  useSaveOpenClawAgentsDefaults,
} from "@/hooks/useOpenClaw";
import { extractErrorMessage } from "@/utils/errorUtils";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import type { OpenClawAgentsDefaults } from "@/types";

const AgentsDefaultsPanel: React.FC = () => {
  const { t } = useTranslation();
  const { data: agentsData, isLoading } = useOpenClawAgentsDefaults();
  const saveAgentsMutation = useSaveOpenClawAgentsDefaults();
  const [defaults, setDefaults] = useState<OpenClawAgentsDefaults | null>(null);
  const [primaryModel, setPrimaryModel] = useState("");
  const [fallbacks, setFallbacks] = useState("");

  // Extra known fields from agents.defaults
  const [workspace, setWorkspace] = useState("");
  const [timeout, setTimeout_] = useState("");
  const [contextTokens, setContextTokens] = useState("");
  const [maxConcurrent, setMaxConcurrent] = useState("");

  useEffect(() => {
    // agentsData is undefined while loading, null when config section is absent
    if (agentsData === undefined) return;
    setDefaults(agentsData);

    if (agentsData) {
      setPrimaryModel(agentsData.model?.primary ?? "");
      setFallbacks((agentsData.model?.fallbacks ?? []).join(", "));

      // Extract known extra fields
      setWorkspace(String(agentsData.workspace ?? ""));
      setTimeout_(String(agentsData.timeout ?? ""));
      setContextTokens(String(agentsData.contextTokens ?? ""));
      setMaxConcurrent(String(agentsData.maxConcurrent ?? ""));
    }
  }, [agentsData]);

  const handleSave = async () => {
    try {
      // Preserve all unknown fields from original data
      const updated: OpenClawAgentsDefaults = { ...defaults };

      // Model configuration
      const fallbackList = fallbacks
        .split(",")
        .map((s) => s.trim())
        .filter(Boolean);

      if (primaryModel.trim()) {
        updated.model = {
          primary: primaryModel.trim(),
          ...(fallbackList.length > 0 ? { fallbacks: fallbackList } : {}),
        };
      }

      // Optional fields
      if (workspace.trim()) updated.workspace = workspace.trim();
      else delete updated.workspace;

      // Numeric fields: validate before saving to avoid NaN
      const parseNum = (v: string) => {
        const n = Number(v);
        return !isNaN(n) && isFinite(n) ? n : undefined;
      };

      const timeoutNum = timeout.trim() ? parseNum(timeout) : undefined;
      if (timeoutNum !== undefined) updated.timeout = timeoutNum;
      else delete updated.timeout;

      const ctxNum = contextTokens.trim() ? parseNum(contextTokens) : undefined;
      if (ctxNum !== undefined) updated.contextTokens = ctxNum;
      else delete updated.contextTokens;

      const concNum = maxConcurrent.trim()
        ? parseNum(maxConcurrent)
        : undefined;
      if (concNum !== undefined) updated.maxConcurrent = concNum;
      else delete updated.maxConcurrent;

      await saveAgentsMutation.mutateAsync(updated);
      toast.success(t("openclaw.agents.saveSuccess"));
    } catch (error) {
      const detail = extractErrorMessage(error);
      toast.error(t("openclaw.agents.saveFailed"), {
        description: detail || undefined,
      });
    }
  };

  if (isLoading) {
    return (
      <div className="px-6 pt-4 pb-8 flex items-center justify-center min-h-[200px]">
        <div className="text-sm text-muted-foreground">
          {t("common.loading")}
        </div>
      </div>
    );
  }

  return (
    <div className="px-6 pt-4 pb-8">
      <p className="text-sm text-muted-foreground mb-6">
        {t("openclaw.agents.description")}
      </p>

      {/* Model Configuration Card */}
      <div className="rounded-xl border border-border bg-card p-5 mb-4">
        <h3 className="text-sm font-medium mb-4">
          {t("openclaw.agents.modelSection")}
        </h3>

        <div className="space-y-4">
          <div>
            <Label className="mb-1.5 block">
              {t("openclaw.agents.primaryModel")}
            </Label>
            <Input
              value={primaryModel}
              onChange={(e) => setPrimaryModel(e.target.value)}
              placeholder="provider/model-id"
              className="font-mono text-xs"
            />
            <p className="text-xs text-muted-foreground mt-1">
              {t("openclaw.agents.primaryModelHint")}
            </p>
          </div>

          <div>
            <Label className="mb-1.5 block">
              {t("openclaw.agents.fallbackModels")}
            </Label>
            <Input
              value={fallbacks}
              onChange={(e) => setFallbacks(e.target.value)}
              placeholder="provider/model-a, provider/model-b"
              className="font-mono text-xs"
            />
            <p className="text-xs text-muted-foreground mt-1">
              {t("openclaw.agents.fallbackModelsHint")}
            </p>
          </div>
        </div>
      </div>

      {/* Runtime Parameters Card */}
      <div className="rounded-xl border border-border bg-card p-5 mb-4">
        <h3 className="text-sm font-medium mb-4">
          {t("openclaw.agents.runtimeSection")}
        </h3>

        <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
          <div>
            <Label className="mb-1.5 block">
              {t("openclaw.agents.workspace")}
            </Label>
            <Input
              value={workspace}
              onChange={(e) => setWorkspace(e.target.value)}
              placeholder="~/projects"
              className="font-mono text-xs"
            />
          </div>

          <div>
            <Label className="mb-1.5 block">
              {t("openclaw.agents.timeout")}
            </Label>
            <Input
              type="number"
              value={timeout}
              onChange={(e) => setTimeout_(e.target.value)}
              placeholder="300"
              className="font-mono text-xs"
            />
          </div>

          <div>
            <Label className="mb-1.5 block">
              {t("openclaw.agents.contextTokens")}
            </Label>
            <Input
              type="number"
              value={contextTokens}
              onChange={(e) => setContextTokens(e.target.value)}
              placeholder="200000"
              className="font-mono text-xs"
            />
          </div>

          <div>
            <Label className="mb-1.5 block">
              {t("openclaw.agents.maxConcurrent")}
            </Label>
            <Input
              type="number"
              value={maxConcurrent}
              onChange={(e) => setMaxConcurrent(e.target.value)}
              placeholder="4"
              className="font-mono text-xs"
            />
          </div>
        </div>
      </div>

      {/* Save button */}
      <div className="flex justify-end">
        <Button
          size="sm"
          onClick={handleSave}
          disabled={saveAgentsMutation.isPending}
        >
          <Save className="w-4 h-4 mr-1" />
          {saveAgentsMutation.isPending ? t("common.saving") : t("common.save")}
        </Button>
      </div>
    </div>
  );
};

export default AgentsDefaultsPanel;
