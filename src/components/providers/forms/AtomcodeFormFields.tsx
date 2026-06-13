import { useTranslation } from "react-i18next";
import { FormLabel } from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { ApiKeySection } from "./shared";
import type { ProviderCategory } from "@/types";

type AtomcodeType = "openai" | "claude" | "ollama";

interface AtomcodeFormFieldsProps {
  // Provider type
  type: AtomcodeType;
  onTypeChange: (type: AtomcodeType) => void;

  // Model
  model: string;
  onModelChange: (model: string) => void;

  // API Key
  apiKey: string;
  onApiKeyChange: (key: string) => void;
  category?: ProviderCategory;
  shouldShowApiKeyLink: boolean;
  websiteUrl: string;
  isPartner?: boolean;
  partnerPromotionKey?: string;

  // Base URL
  baseUrl: string;
  onBaseUrlChange: (url: string) => void;

  // Context window (optional)
  contextWindow: number | undefined;
  onContextWindowChange: (val: number | undefined) => void;

  // Thinking (only when type === "claude")
  thinkingEnabled: boolean;
  onThinkingEnabledChange: (enabled: boolean) => void;
  thinkingBudget: number | undefined;
  onThinkingBudgetChange: (budget: number | undefined) => void;
}

const ATOMCODE_TYPE_OPTIONS: { value: AtomcodeType; labelKey: string; defaultLabel: string }[] = [
  { value: "openai", labelKey: "atomcode.form.typeOpenai", defaultLabel: "OpenAI" },
  { value: "claude", labelKey: "atomcode.form.typeClaude", defaultLabel: "Claude" },
  { value: "ollama", labelKey: "atomcode.form.typeOllama", defaultLabel: "Ollama" },
];

export function AtomcodeFormFields({
  type,
  onTypeChange,
  model,
  onModelChange,
  apiKey,
  onApiKeyChange,
  category,
  shouldShowApiKeyLink,
  websiteUrl,
  isPartner,
  partnerPromotionKey,
  baseUrl,
  onBaseUrlChange,
  contextWindow,
  onContextWindowChange,
  thinkingEnabled,
  onThinkingEnabledChange,
  thinkingBudget,
  onThinkingBudgetChange,
}: AtomcodeFormFieldsProps) {
  const { t } = useTranslation();

  return (
    <>
      {/* Type selector */}
      <div className="space-y-2">
        <FormLabel htmlFor="atomcode-type">
          {t("atomcode.form.type", { defaultValue: "API 类型" })}
        </FormLabel>
        <Select
          value={type}
          onValueChange={(v) => onTypeChange(v as AtomcodeType)}
        >
          <SelectTrigger id="atomcode-type">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {ATOMCODE_TYPE_OPTIONS.map((opt) => (
              <SelectItem key={opt.value} value={opt.value}>
                {t(opt.labelKey, { defaultValue: opt.defaultLabel })}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <p className="text-xs text-muted-foreground">
          {t("atomcode.form.typeHint", {
            defaultValue: "供应商 API 类型。请根据接入端点选择正确的协议。",
          })}
        </p>
      </div>

      {/* Model */}
      <div className="space-y-2">
        <FormLabel htmlFor="atomcode-model">
          {t("atomcode.form.model", { defaultValue: "模型" })}
        </FormLabel>
        <Input
          id="atomcode-model"
          value={model}
          onChange={(e) => onModelChange(e.target.value)}
          placeholder={
            type === "claude"
              ? "claude-opus-4-6"
              : type === "ollama"
                ? "llama3.2"
                : "gpt-4o"
          }
        />
        <p className="text-xs text-muted-foreground">
          {t("atomcode.form.modelHint", {
            defaultValue: "切换到此供应商时使用的默认模型 ID。",
          })}
        </p>
      </div>

      {/* API Key */}
      <ApiKeySection
        value={apiKey}
        onChange={onApiKeyChange}
        category={category}
        shouldShowLink={shouldShowApiKeyLink}
        websiteUrl={websiteUrl}
        isPartner={isPartner}
        partnerPromotionKey={partnerPromotionKey}
      />

      {/* Base URL */}
      <div className="space-y-2">
        <FormLabel htmlFor="atomcode-base-url">
          {t("atomcode.form.baseUrl", { defaultValue: "API 端点" })}
        </FormLabel>
        <Input
          id="atomcode-base-url"
          value={baseUrl}
          onChange={(e) => onBaseUrlChange(e.target.value)}
          placeholder={
            type === "claude"
              ? "https://api.anthropic.com"
              : type === "ollama"
                ? "http://localhost:11434"
                : "https://api.openai.com/v1"
          }
        />
        <p className="text-xs text-muted-foreground">
          {t("atomcode.form.baseUrlHint", {
            defaultValue: "供应商的 API 端点地址（可选）。",
          })}
        </p>
      </div>

      {/* Context window (optional) */}
      <div className="space-y-2">
        <FormLabel htmlFor="atomcode-context-window">
          {t("atomcode.form.contextWindow", { defaultValue: "上下文长度（可选）" })}
        </FormLabel>
        <Input
          id="atomcode-context-window"
          type="number"
          min={1}
          value={contextWindow ?? ""}
          onChange={(e) => {
            const v = e.target.value;
            if (v === "") {
              onContextWindowChange(undefined);
              return;
            }
            const n = parseInt(v, 10);
            onContextWindowChange(Number.isFinite(n) && n > 0 ? n : undefined);
          }}
          placeholder="200000"
        />
        <p className="text-xs text-muted-foreground">
          {t("atomcode.form.contextWindowHint", {
            defaultValue: "模型最大上下文 token 数（可选）。留空表示不限制。",
          })}
        </p>
      </div>

      {/* Thinking fields — only shown when type === "claude" */}
      {type === "claude" && (
        <>
          <div className="flex items-center justify-between space-x-2">
            <div className="space-y-0.5">
              <FormLabel htmlFor="atomcode-thinking-enabled">
                {t("atomcode.form.thinkingEnabled", {
                  defaultValue: "启用思考模式",
                })}
              </FormLabel>
              <p className="text-xs text-muted-foreground">
                {t("atomcode.form.thinkingEnabledHint", {
                  defaultValue: "启用 Claude 的扩展思考功能（extended thinking）。",
                })}
              </p>
            </div>
            <Switch
              id="atomcode-thinking-enabled"
              checked={thinkingEnabled}
              onCheckedChange={onThinkingEnabledChange}
            />
          </div>

          {thinkingEnabled && (
            <div className="space-y-2">
              <FormLabel htmlFor="atomcode-thinking-budget">
                {t("atomcode.form.thinkingBudget", {
                  defaultValue: "思考 Token 预算（可选）",
                })}
              </FormLabel>
              <Input
                id="atomcode-thinking-budget"
                type="number"
                min={1}
                value={thinkingBudget ?? ""}
                onChange={(e) => {
                  const v = e.target.value;
                  if (v === "") {
                    onThinkingBudgetChange(undefined);
                    return;
                  }
                  const n = parseInt(v, 10);
                  onThinkingBudgetChange(
                    Number.isFinite(n) && n > 0 ? n : undefined,
                  );
                }}
                placeholder="10000"
              />
              <p className="text-xs text-muted-foreground">
                {t("atomcode.form.thinkingBudgetHint", {
                  defaultValue:
                    "思考步骤允许使用的最大 token 数（可选）。留空表示使用默认值。",
                })}
              </p>
            </div>
          )}
        </>
      )}
    </>
  );
}
