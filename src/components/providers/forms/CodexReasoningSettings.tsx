import { useTranslation } from "react-i18next";
import { Switch } from "@/components/ui/switch";
import { Textarea } from "@/components/ui/textarea";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import type {
  CodexReasoningContinuationConfig,
  CodexSystemPromptConfig,
} from "@/types";

export interface CodexReasoningSettingsProps {
  systemPrompt: CodexSystemPromptConfig;
  onSystemPromptChange: (value: CodexSystemPromptConfig) => void;
  continuation: CodexReasoningContinuationConfig;
  onContinuationChange: (value: CodexReasoningContinuationConfig) => void;
}

/**
 * Provider-level Codex system-prompt replacement + reasoning continuation.
 * Prompt text is never echoed to toast/error/log by callers — only stored in meta.
 */
export function CodexReasoningSettings({
  systemPrompt,
  onSystemPromptChange,
  continuation,
  onContinuationChange,
}: CodexReasoningSettingsProps) {
  const { t } = useTranslation();

  return (
    <div
      className="space-y-4 rounded-lg border border-border/60 p-4"
      data-testid="codex-reasoning-settings"
    >
      <div>
        <h4 className="text-sm font-medium">
          {t("codexConfig.systemPromptSection", {
            defaultValue: "系统提示词替换",
          })}
        </h4>
        <p className="mt-1 text-xs text-muted-foreground">
          {t("codexConfig.systemPromptHint", {
            defaultValue:
              "启用后完整替换 Codex 系统层（instructions / system / developer）。不会修改用户消息。",
          })}
        </p>
      </div>

      <div className="flex items-center justify-between gap-3">
        <Label htmlFor="codex-system-prompt-enabled" className="text-sm">
          {t("codexConfig.systemPromptToggle", {
            defaultValue: "启用系统提示词替换",
          })}
        </Label>
        <Switch
          id="codex-system-prompt-enabled"
          checked={systemPrompt.enabled}
          onCheckedChange={(enabled) =>
            onSystemPromptChange({ ...systemPrompt, enabled })
          }
          aria-label={t("codexConfig.systemPromptToggle", {
            defaultValue: "启用系统提示词替换",
          })}
        />
      </div>

      {systemPrompt.enabled && (
        <div className="space-y-3">
          <div className="space-y-1.5">
            <Label htmlFor="codex-system-prompt-text" className="text-sm">
              {t("codexConfig.systemPromptReplacement", {
                defaultValue: "替换内容",
              })}
            </Label>
            <Textarea
              id="codex-system-prompt-text"
              value={systemPrompt.replacement}
              onChange={(e) =>
                onSystemPromptChange({
                  ...systemPrompt,
                  replacement: e.target.value,
                })
              }
              rows={5}
              className="font-mono text-xs"
              placeholder={t("codexConfig.systemPromptPlaceholder", {
                defaultValue: "You are a helpful coding assistant…",
              })}
              data-testid="codex-system-prompt-textarea"
            />
          </div>

          <div className="flex items-center justify-between gap-3">
            <div>
              <Label htmlFor="codex-correct-model-identity" className="text-sm">
                {t("codexConfig.correctModelIdentity", {
                  defaultValue: "纠正模型身份表述",
                })}
              </Label>
              <p className="text-xs text-muted-foreground">
                {t("codexConfig.correctModelIdentityHint", {
                  defaultValue:
                    "将替换文本中的 GPT-4 / Claude 等身份纠正为当前上游模型名。",
                })}
              </p>
            </div>
            <Switch
              id="codex-correct-model-identity"
              checked={systemPrompt.correctModelIdentity !== false}
              onCheckedChange={(correctModelIdentity) =>
                onSystemPromptChange({
                  ...systemPrompt,
                  correctModelIdentity,
                })
              }
            />
          </div>
        </div>
      )}

      <div className="border-t border-border/40 pt-4">
        <div className="flex items-center justify-between gap-3">
          <div>
            <Label htmlFor="codex-continuation-enabled" className="text-sm">
              {t("codexConfig.continuationToggle", {
                defaultValue: "推理续接",
              })}
            </Label>
            <p className="text-xs text-muted-foreground">
              {t("codexConfig.continuationHint", {
                defaultValue:
                  "在输出被截断时自动续接（与系统提示词开关相互独立）。",
              })}
            </p>
          </div>
          <Switch
            id="codex-continuation-enabled"
            checked={continuation.enabled}
            onCheckedChange={(enabled) =>
              onContinuationChange({ ...continuation, enabled })
            }
            aria-label={t("codexConfig.continuationToggle", {
              defaultValue: "推理续接",
            })}
          />
        </div>

        {continuation.enabled && (
          <div className="mt-3 space-y-1.5">
            <Label htmlFor="codex-continuation-max-rounds" className="text-sm">
              {t("codexConfig.continuationMaxRounds", {
                defaultValue: "最大续接轮数 (1–3)",
              })}
            </Label>
            <Input
              id="codex-continuation-max-rounds"
              type="number"
              min={1}
              max={3}
              inputMode="numeric"
              value={continuation.maxRounds ?? 3}
              onChange={(e) => {
                const raw = Number(e.target.value);
                const clamped = Number.isFinite(raw)
                  ? Math.min(3, Math.max(1, Math.trunc(raw)))
                  : 3;
                onContinuationChange({
                  ...continuation,
                  maxRounds: clamped,
                });
              }}
              className="w-24"
              data-testid="codex-continuation-max-rounds"
            />
          </div>
        )}
      </div>
    </div>
  );
}

export default CodexReasoningSettings;
