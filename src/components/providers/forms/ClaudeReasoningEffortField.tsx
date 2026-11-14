import { useTranslation } from "react-i18next";
import { Label } from "@/components/ui/label";
import { HelpTip } from "@/components/ui/help-tip";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import type { ClaudeReasoningEffortMode } from "@/types";

export type ClaudeReasoningEffortChoice = ClaudeReasoningEffortMode | "auto";

const CHOICES: ClaudeReasoningEffortChoice[] = ["auto", "passthrough", "off"];

export function toClaudeReasoningEffortChoice(
  value: string | undefined,
): ClaudeReasoningEffortChoice {
  return CHOICES.includes(value as ClaudeReasoningEffortChoice)
    ? (value as ClaudeReasoningEffortChoice)
    : "auto";
}

interface ClaudeReasoningEffortFieldProps {
  /** 下拉框的 id（用于 label htmlFor）；两个表单需传入各自唯一值。 */
  id: string;
  value: ClaudeReasoningEffortChoice;
  onChange: (value: ClaudeReasoningEffortChoice) => void;
}

/**
 * Claude 路由转成 OpenAI Chat / Responses 时思考档位的发法（Claude Code / Claude
 * Desktop 表单共用）。后端判定见 `proxy/providers/claude_reasoning.rs`。
 */
export function ClaudeReasoningEffortField({
  id,
  value,
  onChange,
}: ClaudeReasoningEffortFieldProps) {
  const { t } = useTranslation();
  const label = t("providerForm.reasoningEffort", {
    defaultValue: "思考档位",
  });
  const optionLabels: Record<ClaudeReasoningEffortChoice, string> = {
    auto: t("providerForm.reasoningEffortAuto", {
      defaultValue: "自动（推荐）",
    }),
    passthrough: t("providerForm.reasoningEffortPassthrough", {
      defaultValue: "原样发送",
    }),
    off: t("providerForm.reasoningEffortOff", { defaultValue: "不发送" }),
  };

  return (
    <div className="space-y-2">
      <div className="flex items-center gap-0.5">
        <Label htmlFor={id}>{label}</Label>
        <HelpTip title={label}>
          {t("providerForm.reasoningEffortHelp", {
            defaultValue:
              "经路由转成 OpenAI 格式时，客户端选的思考档位怎么发给上游。自动：按上游支持的档位发，认不出的上游最多发 high，关掉思考时不发。原样发送：客户端选哪档发哪档，上游不支持 xhigh 或 max 时会报错。不发送：上游因这个参数报错时用。",
          })}
        </HelpTip>
      </div>
      <Select
        value={value}
        onValueChange={(next) => {
          // 表单里 Radix Select 可能先回报空串，忽略它，免得清掉已有的值。
          if (next) onChange(next as ClaudeReasoningEffortChoice);
        }}
      >
        <SelectTrigger id={id} className="w-full">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {CHOICES.map((choice) => (
            <SelectItem key={choice} value={choice}>
              {optionLabels[choice]}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </div>
  );
}
