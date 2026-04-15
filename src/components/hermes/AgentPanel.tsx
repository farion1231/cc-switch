import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Save } from "lucide-react";
import { toast } from "sonner";
import {
  useHermesAgentConfig,
  useSaveHermesAgentConfig,
} from "@/hooks/useHermes";
import { extractErrorMessage } from "@/utils/errorUtils";
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
import type { HermesAgentConfig } from "@/types";

const UNSET_SENTINEL = "__unset__";

const REASONING_EFFORT_OPTIONS = [
  { value: UNSET_SENTINEL, labelKey: "hermes.agent.notSet" },
  { value: "none", label: "None" },
  { value: "minimal", label: "Minimal" },
  { value: "low", label: "Low" },
  { value: "medium", label: "Medium" },
  { value: "high", label: "High" },
  { value: "xhigh", label: "Extra High" },
] as const;

const APPROVALS_MODE_OPTIONS = [
  { value: UNSET_SENTINEL, labelKey: "hermes.agent.notSet" },
  { value: "manual", label: "Manual" },
  { value: "smart", label: "Smart" },
  { value: "off", label: "Off" },
] as const;

const AgentPanel: React.FC = () => {
  const { t } = useTranslation();
  const { data: agentData, isLoading } = useHermesAgentConfig();
  const saveAgentMutation = useSaveHermesAgentConfig();

  const [maxTurns, setMaxTurns] = useState("");
  const [reasoningEffort, setReasoningEffort] = useState(UNSET_SENTINEL);
  const [toolUseEnforcement, setToolUseEnforcement] = useState("");
  const [approvalsMode, setApprovalsMode] = useState(UNSET_SENTINEL);

  // Preserve unknown fields
  const [extra, setExtra] = useState<Record<string, unknown>>({});

  useEffect(() => {
    if (agentData === undefined) return;
    if (agentData) {
      setMaxTurns(
        agentData.max_turns != null ? String(agentData.max_turns) : "",
      );
      setReasoningEffort(agentData.reasoning_effort ?? UNSET_SENTINEL);
      setToolUseEnforcement(
        agentData.tool_use_enforcement != null
          ? typeof agentData.tool_use_enforcement === "string"
            ? agentData.tool_use_enforcement
            : JSON.stringify(agentData.tool_use_enforcement)
          : "",
      );
      setApprovalsMode(agentData.approvals_mode ?? UNSET_SENTINEL);
      const {
        max_turns: _mt,
        reasoning_effort: _re,
        tool_use_enforcement: _tu,
        approvals_mode: _am,
        ...rest
      } = agentData;
      setExtra(rest);
    } else {
      setMaxTurns("");
      setReasoningEffort(UNSET_SENTINEL);
      setToolUseEnforcement("");
      setApprovalsMode(UNSET_SENTINEL);
      setExtra({});
    }
  }, [agentData]);

  const handleSave = async () => {
    try {
      const config: HermesAgentConfig = {
        ...extra,
      };

      const mt = parseInt(maxTurns);
      if (!isNaN(mt) && mt > 0) config.max_turns = mt;

      if (reasoningEffort !== UNSET_SENTINEL) {
        config.reasoning_effort = reasoningEffort;
      }

      if (toolUseEnforcement.trim()) {
        // Try parsing as JSON (for boolean/array values)
        try {
          config.tool_use_enforcement = JSON.parse(toolUseEnforcement.trim());
        } catch {
          config.tool_use_enforcement = toolUseEnforcement.trim();
        }
      }

      if (approvalsMode !== UNSET_SENTINEL) {
        config.approvals_mode = approvalsMode;
      }

      await saveAgentMutation.mutateAsync(config);
      toast.success(t("hermes.agent.saveSuccess"));
    } catch (error) {
      toast.error(t("hermes.agent.saveFailed"), {
        description: extractErrorMessage(error),
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
      <p className="text-sm text-muted-foreground mb-4">
        {t("hermes.agent.description")}
      </p>

      <div className="rounded-xl border border-border bg-card p-5 mb-4">
        <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
          <div className="space-y-2">
            <Label htmlFor="hermes-agent-maxturns">
              {t("hermes.agent.maxTurns", { defaultValue: "Max Turns" })}
            </Label>
            <Input
              id="hermes-agent-maxturns"
              type="number"
              value={maxTurns}
              onChange={(e) => setMaxTurns(e.target.value)}
              placeholder="100"
            />
            <p className="text-xs text-muted-foreground">
              {t("hermes.agent.maxTurnsHint", {
                defaultValue: "Maximum number of agent turns per session",
              })}
            </p>
          </div>

          <div className="space-y-2">
            <Label htmlFor="hermes-agent-reasoning">
              {t("hermes.agent.reasoningEffort", {
                defaultValue: "Reasoning Effort",
              })}
            </Label>
            <Select value={reasoningEffort} onValueChange={setReasoningEffort}>
              <SelectTrigger id="hermes-agent-reasoning">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {REASONING_EFFORT_OPTIONS.map((opt) => (
                  <SelectItem key={opt.value} value={opt.value}>
                    {"label" in opt
                      ? opt.label
                      : t(opt.labelKey, { defaultValue: "Not set" })}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <p className="text-xs text-muted-foreground">
              {t("hermes.agent.reasoningEffortHint", {
                defaultValue:
                  "Controls the depth of reasoning: none, minimal, low, medium, high, xhigh",
              })}
            </p>
          </div>

          <div className="space-y-2">
            <Label htmlFor="hermes-agent-tooluse">
              {t("hermes.agent.toolUseEnforcement", {
                defaultValue: "Tool Use Enforcement",
              })}
            </Label>
            <Input
              id="hermes-agent-tooluse"
              value={toolUseEnforcement}
              onChange={(e) => setToolUseEnforcement(e.target.value)}
              placeholder="auto"
            />
            <p className="text-xs text-muted-foreground">
              {t("hermes.agent.toolUseHint", {
                defaultValue:
                  'Values: "auto", true, false, or a JSON array of tool names',
              })}
            </p>
          </div>

          <div className="space-y-2">
            <Label htmlFor="hermes-agent-approvals">
              {t("hermes.agent.approvalsMode", {
                defaultValue: "Approvals Mode",
              })}
            </Label>
            <Select value={approvalsMode} onValueChange={setApprovalsMode}>
              <SelectTrigger id="hermes-agent-approvals">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {APPROVALS_MODE_OPTIONS.map((opt) => (
                  <SelectItem key={opt.value} value={opt.value}>
                    {"label" in opt
                      ? opt.label
                      : t(opt.labelKey, { defaultValue: "Not set" })}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <p className="text-xs text-muted-foreground">
              {t("hermes.agent.approvalsModeHint", {
                defaultValue:
                  "Controls tool call approval: manual (always ask), smart (auto-approve safe), off (never ask)",
              })}
            </p>
          </div>
        </div>
      </div>

      <div className="flex justify-end">
        <Button
          size="sm"
          onClick={handleSave}
          disabled={saveAgentMutation.isPending}
        >
          <Save className="w-4 h-4 mr-1" />
          {saveAgentMutation.isPending ? t("common.saving") : t("common.save")}
        </Button>
      </div>
    </div>
  );
};

export default AgentPanel;
