import { useMemo, useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { FormLabel } from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import { ApiKeySection } from "./shared";
import {
  ChevronDown,
  ChevronRight,
  Plus,
  Trash2,
  RotateCcw,
} from "lucide-react";
import {
  kimiCapabilities,
  kimiProviderTypes,
  type KimiCapability,
  type KimiModelEntry,
} from "@/config/kimiProviderPresets";
import type { ProviderCategory } from "@/types";
import type { KimiFormState } from "./hooks/useKimiFormState";

interface KimiFormFieldsProps extends KimiFormState {
  category?: ProviderCategory;
  shouldShowApiKeyLink: boolean;
  websiteUrl: string;
  isPartner?: boolean;
  partnerPromotionKey?: string;
}

interface AdvancedSectionProps {
  open: boolean;
  onOpenChange: (next: boolean) => void;
  label: string;
  children: ReactNode;
}

function AdvancedSection({
  open,
  onOpenChange,
  label,
  children,
}: AdvancedSectionProps) {
  return (
    <Collapsible open={open} onOpenChange={onOpenChange}>
      <CollapsibleTrigger asChild>
        <Button
          type="button"
          variant="ghost"
          size="sm"
          className="h-7 gap-1 text-xs text-muted-foreground hover:text-foreground"
        >
          {open ? (
            <ChevronDown className="h-3.5 w-3.5" />
          ) : (
            <ChevronRight className="h-3.5 w-3.5" />
          )}
          {label}
        </Button>
      </CollapsibleTrigger>
      <CollapsibleContent className="space-y-3 pt-2">
        {children}
      </CollapsibleContent>
    </Collapsible>
  );
}

function parseNumberInput(value: string): number | undefined {
  if (!value.trim()) return undefined;
  const parsed = Number.parseInt(value, 10);
  return Number.isFinite(parsed) ? parsed : undefined;
}

function BooleanSelect({
  value,
  onChange,
  id,
}: {
  value: boolean | undefined;
  onChange: (value: boolean | undefined) => void;
  id: string;
}) {
  const { t } = useTranslation();
  return (
    <Select
      value={value === undefined ? "unset" : value ? "true" : "false"}
      onValueChange={(next) =>
        onChange(next === "unset" ? undefined : next === "true")
      }
    >
      <SelectTrigger id={id}>
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        <SelectItem value="unset">
          {t("kimi.form.unset", { defaultValue: "不写入" })}
        </SelectItem>
        <SelectItem value="true">
          {t("common.enabled", { defaultValue: "启用" })}
        </SelectItem>
        <SelectItem value="false">
          {t("common.disabled", { defaultValue: "禁用" })}
        </SelectItem>
      </SelectContent>
    </Select>
  );
}

function KeyValueEditor({
  label,
  value,
  onChange,
  keyPlaceholder,
  valuePlaceholder,
}: {
  label: string;
  value: Record<string, string>;
  onChange: (value: Record<string, string>) => void;
  keyPlaceholder: string;
  valuePlaceholder: string;
}) {
  const { t } = useTranslation();
  const entries = useMemo(() => Object.entries(value), [value]);

  const updateEntry = (index: number, key: string, nextValue: string) => {
    const nextEntries = [...entries];
    nextEntries[index] = [key, nextValue];
    onChange(
      Object.fromEntries(
        nextEntries
          .filter(([entryKey]) => entryKey.trim())
          .map(([k, v]) => [k.trim(), v]),
      ),
    );
  };

  const removeEntry = (index: number) => {
    const nextEntries = [...entries];
    nextEntries.splice(index, 1);
    onChange(Object.fromEntries(nextEntries));
  };

  return (
    <div className="space-y-2">
      <div className="flex items-center justify-between">
        <label className="text-xs text-muted-foreground">{label}</label>
        <Button
          type="button"
          variant="outline"
          size="sm"
          className="h-7 gap-1"
          onClick={() => onChange({ ...value, "": "" })}
        >
          <Plus className="h-3.5 w-3.5" />
          {t("common.add", { defaultValue: "添加" })}
        </Button>
      </div>
      {entries.length === 0 ? (
        <p className="text-xs text-muted-foreground">
          {t("kimi.form.noKeyValueRows", { defaultValue: "暂无条目。" })}
        </p>
      ) : (
        <div className="space-y-2">
          {entries.map(([entryKey, entryValue], index) => (
            <div key={`${entryKey}-${index}`} className="flex gap-2">
              <Input
                value={entryKey}
                onChange={(event) =>
                  updateEntry(index, event.target.value, entryValue)
                }
                placeholder={keyPlaceholder}
              />
              <Input
                value={entryValue}
                onChange={(event) =>
                  updateEntry(index, entryKey, event.target.value)
                }
                placeholder={valuePlaceholder}
              />
              <Button
                type="button"
                variant="ghost"
                size="icon"
                className="shrink-0 text-muted-foreground hover:text-destructive"
                onClick={() => removeEntry(index)}
              >
                <Trash2 className="h-4 w-4" />
              </Button>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

export function KimiFormFields({
  category,
  shouldShowApiKeyLink,
  websiteUrl,
  isPartner,
  partnerPromotionKey,
  kimiProviderType,
  kimiBaseUrl,
  kimiApiKey,
  kimiOauth,
  kimiEnv,
  kimiCustomHeaders,
  kimiModels,
  kimiDefaultModel,
  kimiDefaultThinking,
  kimiDefaultPermissionMode,
  kimiDefaultPlanMode,
  kimiMergeAllAvailableSkills,
  kimiTelemetry,
  kimiThinkingMode,
  kimiThinkingEffort,
  kimiMaxRetriesPerStep,
  kimiReservedContextSize,
  kimiMaxRunningTasks,
  kimiKeepAliveOnExit,
  kimiMicroCompaction,
  handleKimiProviderTypeChange,
  handleKimiBaseUrlChange,
  handleKimiApiKeyChange,
  handleKimiOauthChange,
  handleKimiEnvChange,
  handleKimiCustomHeadersChange,
  handleKimiModelsChange,
  handleKimiDefaultModelChange,
  handleKimiDefaultThinkingChange,
  handleKimiDefaultPermissionModeChange,
  handleKimiDefaultPlanModeChange,
  handleKimiMergeAllAvailableSkillsChange,
  handleKimiTelemetryChange,
  handleKimiThinkingModeChange,
  handleKimiThinkingEffortChange,
  handleKimiMaxRetriesPerStepChange,
  handleKimiReservedContextSizeChange,
  handleKimiMaxRunningTasksChange,
  handleKimiKeepAliveOnExitChange,
  handleKimiMicroCompactionChange,
}: KimiFormFieldsProps) {
  const { t } = useTranslation();
  const [expandedModels, setExpandedModels] = useState<Record<number, boolean>>(
    {},
  );
  const [providerAdvancedOpen, setProviderAdvancedOpen] = useState(
    Object.keys(kimiEnv).length > 0 ||
      Object.keys(kimiCustomHeaders).length > 0,
  );
  const [generalAdvancedOpen, setGeneralAdvancedOpen] = useState(false);
  const modelKeysRef = useRef<string[]>([]);

  while (modelKeysRef.current.length < kimiModels.length) {
    modelKeysRef.current.push(crypto.randomUUID());
  }
  if (modelKeysRef.current.length > kimiModels.length) {
    modelKeysRef.current.length = kimiModels.length;
  }

  const updateModel = (
    index: number,
    field: keyof KimiModelEntry,
    value: unknown,
  ) => {
    const next = [...kimiModels];
    next[index] = { ...next[index], [field]: value };
    handleKimiModelsChange(next);
  };

  const addModel = () => {
    modelKeysRef.current.push(crypto.randomUUID());
    handleKimiModelsChange([
      ...kimiModels,
      {
        id: "",
        provider: "",
        model: "",
        capabilities: ["tool_use"],
      },
    ]);
  };

  const removeModel = (index: number) => {
    modelKeysRef.current.splice(index, 1);
    const next = [...kimiModels];
    next.splice(index, 1);
    handleKimiModelsChange(next);
  };

  const setDefaultToFirstModel = () => {
    const firstModelId = kimiModels.find((model) => model.id.trim())?.id ?? "";
    handleKimiDefaultModelChange(firstModelId);
  };

  return (
    <>
      <div className="space-y-2">
        <FormLabel htmlFor="kimi-provider-type">
          {t("kimi.form.providerType", { defaultValue: "Provider 类型" })}
        </FormLabel>
        <Select
          value={kimiProviderType}
          onValueChange={handleKimiProviderTypeChange}
        >
          <SelectTrigger id="kimi-provider-type">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {kimiProviderTypes.map((type) => (
              <SelectItem key={type.value} value={type.value}>
                {type.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <p className="text-xs text-muted-foreground">
          {t("kimi.form.providerTypeHint", {
            defaultValue: "对应 Kimi Code config.toml 中 provider.type。",
          })}
        </p>
      </div>

      <div className="space-y-2">
        <FormLabel htmlFor="kimi-base-url">
          {t("kimi.form.baseUrl", { defaultValue: "请求地址" })}
        </FormLabel>
        <Input
          id="kimi-base-url"
          value={kimiBaseUrl}
          onChange={(event) => handleKimiBaseUrlChange(event.target.value)}
          placeholder="https://api.kimi.com/coding/v1"
        />
        <p className="text-xs text-muted-foreground">
          {t("kimi.form.baseUrlHint", {
            defaultValue:
              "写入 provider.base_url。官方 Kimi Code 使用 /coding/v1。",
          })}
        </p>
      </div>

      <ApiKeySection
        id="kimi-api-key"
        value={kimiApiKey}
        onChange={handleKimiApiKeyChange}
        category={category === "official" ? undefined : category}
        shouldShowLink={shouldShowApiKeyLink}
        websiteUrl={websiteUrl}
        isPartner={isPartner}
        partnerPromotionKey={partnerPromotionKey}
        placeholder={{
          official: t("providerForm.apiKeyAutoFill", {
            defaultValue: "输入 API Key，将自动填充到配置",
          }),
          thirdParty: t("providerForm.apiKeyAutoFill", {
            defaultValue: "输入 API Key，将自动填充到配置",
          }),
        }}
      />

      <div className="space-y-2">
        <div className="flex items-center justify-between">
          <div className="space-y-0.5">
            <FormLabel>
              {t("kimi.form.oauth", { defaultValue: "OAuth" })}
            </FormLabel>
            <p className="text-xs text-muted-foreground">
              {t("kimi.form.oauthHint", {
                defaultValue:
                  "写入 provider.oauth，适用于支持 OAuth 的 provider。",
              })}
            </p>
          </div>
          <Switch checked={kimiOauth} onCheckedChange={handleKimiOauthChange} />
        </div>
      </div>

      <div className="space-y-2">
        <div className="flex items-center justify-between gap-2">
          <FormLabel htmlFor="kimi-default-model">
            {t("kimi.form.defaultModel", { defaultValue: "默认模型" })}
          </FormLabel>
          <Button
            type="button"
            variant="outline"
            size="sm"
            className="h-7 gap-1"
            onClick={setDefaultToFirstModel}
          >
            <RotateCcw className="h-3.5 w-3.5" />
            {t("kimi.form.useFirstModel", { defaultValue: "使用首个模型" })}
          </Button>
        </div>
        <Input
          id="kimi-default-model"
          value={kimiDefaultModel}
          onChange={(event) => handleKimiDefaultModelChange(event.target.value)}
          placeholder="kimi-code/kimi-for-coding"
        />
      </div>

      <div className="space-y-3">
        <div className="flex items-center justify-between">
          <FormLabel>
            {t("kimi.form.models", { defaultValue: "模型列表" })}
          </FormLabel>
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={addModel}
            className="h-7 gap-1"
          >
            <Plus className="h-3.5 w-3.5" />
            {t("kimi.form.addModel", { defaultValue: "添加模型" })}
          </Button>
        </div>

        {kimiModels.length === 0 ? (
          <p className="py-2 text-sm text-muted-foreground">
            {t("kimi.form.noModels", {
              defaultValue:
                "暂无模型配置。至少添加一个模型并设为 default_model。",
            })}
          </p>
        ) : (
          <div className="space-y-4">
            {kimiModels.map((model, index) => (
              <div
                key={modelKeysRef.current[index]}
                className="space-y-3 rounded-lg border border-border/50 p-3"
              >
                <div className="flex items-center">
                  <span
                    className={`rounded px-1.5 py-0.5 text-[10px] font-medium ${
                      model.id === kimiDefaultModel
                        ? "bg-blue-500/15 text-blue-600 dark:text-blue-400"
                        : "bg-muted text-muted-foreground"
                    }`}
                  >
                    {model.id === kimiDefaultModel
                      ? t("kimi.form.primaryModel", {
                          defaultValue: "默认模型",
                        })
                      : t("kimi.form.fallbackModel", {
                          defaultValue: "可用模型",
                        })}
                  </span>
                </div>

                <div className="flex items-center gap-2">
                  <div className="flex-1 space-y-1">
                    <label className="text-xs text-muted-foreground">
                      {t("kimi.form.modelId", { defaultValue: "模型 ID" })}
                    </label>
                    <Input
                      value={model.id}
                      onChange={(event) =>
                        updateModel(index, "id", event.target.value)
                      }
                      placeholder="kimi-code/kimi-for-coding"
                    />
                  </div>
                  <div className="flex-1 space-y-1">
                    <label className="text-xs text-muted-foreground">
                      {t("kimi.form.upstreamModel", {
                        defaultValue: "上游模型名",
                      })}
                    </label>
                    <Input
                      value={model.model}
                      onChange={(event) =>
                        updateModel(index, "model", event.target.value)
                      }
                      placeholder="kimi-for-coding"
                    />
                  </div>
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon"
                    onClick={() => removeModel(index)}
                    className="mt-5 h-9 w-9 text-muted-foreground hover:text-destructive"
                  >
                    <Trash2 className="h-4 w-4" />
                  </Button>
                </div>

                <AdvancedSection
                  open={expandedModels[index] ?? false}
                  onOpenChange={() =>
                    setExpandedModels((prev) => ({
                      ...prev,
                      [index]: !prev[index],
                    }))
                  }
                  label={t("kimi.form.modelAdvanced", {
                    defaultValue: "模型高级配置",
                  })}
                >
                  <div className="grid gap-2 md:grid-cols-2">
                    <div className="space-y-1">
                      <label className="text-xs text-muted-foreground">
                        {t("kimi.form.modelProvider", {
                          defaultValue: "所属 Provider",
                        })}
                      </label>
                      <Input
                        value={model.provider}
                        onChange={(event) =>
                          updateModel(index, "provider", event.target.value)
                        }
                        placeholder="managed:kimi-code"
                      />
                    </div>
                    <div className="space-y-1">
                      <label className="text-xs text-muted-foreground">
                        {t("kimi.form.displayName", {
                          defaultValue: "显示名称",
                        })}
                      </label>
                      <Input
                        value={model.display_name ?? ""}
                        onChange={(event) =>
                          updateModel(
                            index,
                            "display_name",
                            event.target.value || undefined,
                          )
                        }
                        placeholder="Kimi For Coding"
                      />
                    </div>
                    <div className="space-y-1">
                      <label className="text-xs text-muted-foreground">
                        {t("kimi.form.maxContextSize", {
                          defaultValue: "上下文窗口",
                        })}
                      </label>
                      <Input
                        type="number"
                        value={model.max_context_size ?? ""}
                        onChange={(event) =>
                          updateModel(
                            index,
                            "max_context_size",
                            parseNumberInput(event.target.value),
                          )
                        }
                        placeholder="262144"
                      />
                    </div>
                    <div className="space-y-1">
                      <label className="text-xs text-muted-foreground">
                        {t("kimi.form.maxOutputSize", {
                          defaultValue: "最大输出 Tokens",
                        })}
                      </label>
                      <Input
                        type="number"
                        value={model.max_output_size ?? ""}
                        onChange={(event) =>
                          updateModel(
                            index,
                            "max_output_size",
                            parseNumberInput(event.target.value),
                          )
                        }
                        placeholder="32000"
                      />
                    </div>
                    <div className="space-y-1">
                      <label className="text-xs text-muted-foreground">
                        {t("kimi.form.reasoningKey", {
                          defaultValue: "Reasoning Key",
                        })}
                      </label>
                      <Input
                        value={model.reasoning_key ?? ""}
                        onChange={(event) =>
                          updateModel(
                            index,
                            "reasoning_key",
                            event.target.value || undefined,
                          )
                        }
                        placeholder="reasoning"
                      />
                    </div>
                    <div className="space-y-1">
                      <label className="text-xs text-muted-foreground">
                        {t("kimi.form.adaptiveThinking", {
                          defaultValue: "自适应 Thinking",
                        })}
                      </label>
                      <div className="flex h-9 items-center gap-2">
                        <Switch
                          checked={model.adaptive_thinking === true}
                          onCheckedChange={(checked) =>
                            updateModel(
                              index,
                              "adaptive_thinking",
                              checked ? true : undefined,
                            )
                          }
                        />
                        <span className="text-xs text-muted-foreground">
                          {model.adaptive_thinking
                            ? t("common.enabled", { defaultValue: "启用" })
                            : t("common.disabled", { defaultValue: "禁用" })}
                        </span>
                      </div>
                    </div>
                  </div>

                  <div className="space-y-1">
                    <label className="text-xs text-muted-foreground">
                      {t("kimi.form.capabilities", {
                        defaultValue: "能力",
                      })}
                    </label>
                    <div className="flex flex-wrap gap-3">
                      {kimiCapabilities.map((capability) => (
                        <label
                          key={capability}
                          className="flex cursor-pointer select-none items-center gap-1.5 text-xs"
                        >
                          <Checkbox
                            checked={(model.capabilities ?? []).includes(
                              capability,
                            )}
                            onCheckedChange={(checked) => {
                              const current = model.capabilities ?? [];
                              const next = checked
                                ? Array.from(new Set([...current, capability]))
                                : current.filter((item) => item !== capability);
                              updateModel(
                                index,
                                "capabilities",
                                next as KimiCapability[],
                              );
                            }}
                          />
                          <span>{capability}</span>
                        </label>
                      ))}
                    </div>
                  </div>
                </AdvancedSection>
              </div>
            ))}
          </div>
        )}
      </div>

      <AdvancedSection
        open={providerAdvancedOpen}
        onOpenChange={setProviderAdvancedOpen}
        label={t("kimi.form.providerAdvanced", {
          defaultValue: "Provider 高级配置",
        })}
      >
        <KeyValueEditor
          label={t("kimi.form.env", { defaultValue: "环境变量 env" })}
          value={kimiEnv}
          onChange={handleKimiEnvChange}
          keyPlaceholder="KIMI_API_KEY"
          valuePlaceholder="value"
        />
        <KeyValueEditor
          label={t("kimi.form.customHeaders", {
            defaultValue: "自定义请求头 custom_headers",
          })}
          value={kimiCustomHeaders}
          onChange={handleKimiCustomHeadersChange}
          keyPlaceholder="X-Header-Name"
          valuePlaceholder="value"
        />
      </AdvancedSection>

      <AdvancedSection
        open={generalAdvancedOpen}
        onOpenChange={setGeneralAdvancedOpen}
        label={t("kimi.form.generalAdvanced", {
          defaultValue: "全局高级配置",
        })}
      >
        <div className="grid gap-3 md:grid-cols-2">
          <div className="space-y-1">
            <label className="text-xs text-muted-foreground">
              default_thinking
            </label>
            <BooleanSelect
              id="kimi-default-thinking"
              value={kimiDefaultThinking}
              onChange={handleKimiDefaultThinkingChange}
            />
          </div>
          <div className="space-y-1">
            <label className="text-xs text-muted-foreground">
              default_plan_mode
            </label>
            <BooleanSelect
              id="kimi-default-plan-mode"
              value={kimiDefaultPlanMode}
              onChange={handleKimiDefaultPlanModeChange}
            />
          </div>
          <div className="space-y-1">
            <label className="text-xs text-muted-foreground">
              default_permission_mode
            </label>
            <Input
              value={kimiDefaultPermissionMode}
              onChange={(event) =>
                handleKimiDefaultPermissionModeChange(event.target.value)
              }
              placeholder="manual"
            />
          </div>
          <div className="space-y-1">
            <label className="text-xs text-muted-foreground">
              merge_all_available_skills
            </label>
            <BooleanSelect
              id="kimi-merge-skills"
              value={kimiMergeAllAvailableSkills}
              onChange={handleKimiMergeAllAvailableSkillsChange}
            />
          </div>
          <div className="space-y-1">
            <label className="text-xs text-muted-foreground">telemetry</label>
            <BooleanSelect
              id="kimi-telemetry"
              value={kimiTelemetry}
              onChange={handleKimiTelemetryChange}
            />
          </div>
          <div className="space-y-1">
            <label className="text-xs text-muted-foreground">
              thinking.mode
            </label>
            <Input
              value={kimiThinkingMode}
              onChange={(event) =>
                handleKimiThinkingModeChange(event.target.value)
              }
              placeholder="auto"
            />
          </div>
          <div className="space-y-1">
            <label className="text-xs text-muted-foreground">
              thinking.effort
            </label>
            <Input
              value={kimiThinkingEffort}
              onChange={(event) =>
                handleKimiThinkingEffortChange(event.target.value)
              }
              placeholder="high"
            />
          </div>
          <div className="space-y-1">
            <label className="text-xs text-muted-foreground">
              loop_control.max_retries_per_step
            </label>
            <Input
              type="number"
              value={kimiMaxRetriesPerStep ?? ""}
              onChange={(event) =>
                handleKimiMaxRetriesPerStepChange(
                  parseNumberInput(event.target.value),
                )
              }
              placeholder="3"
            />
          </div>
          <div className="space-y-1">
            <label className="text-xs text-muted-foreground">
              loop_control.reserved_context_size
            </label>
            <Input
              type="number"
              value={kimiReservedContextSize ?? ""}
              onChange={(event) =>
                handleKimiReservedContextSizeChange(
                  parseNumberInput(event.target.value),
                )
              }
              placeholder="50000"
            />
          </div>
          <div className="space-y-1">
            <label className="text-xs text-muted-foreground">
              background.max_running_tasks
            </label>
            <Input
              type="number"
              value={kimiMaxRunningTasks ?? ""}
              onChange={(event) =>
                handleKimiMaxRunningTasksChange(
                  parseNumberInput(event.target.value),
                )
              }
              placeholder="4"
            />
          </div>
          <div className="space-y-1">
            <label className="text-xs text-muted-foreground">
              background.keep_alive_on_exit
            </label>
            <BooleanSelect
              id="kimi-keep-alive-on-exit"
              value={kimiKeepAliveOnExit}
              onChange={handleKimiKeepAliveOnExitChange}
            />
          </div>
          <div className="space-y-1">
            <label className="text-xs text-muted-foreground">
              experimental.micro_compaction
            </label>
            <BooleanSelect
              id="kimi-micro-compaction"
              value={kimiMicroCompaction}
              onChange={handleKimiMicroCompactionChange}
            />
          </div>
        </div>
      </AdvancedSection>
    </>
  );
}
