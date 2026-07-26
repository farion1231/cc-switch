import { useEffect, useState } from "react";
import { ChevronDown, ChevronRight, Plus, RefreshCw } from "lucide-react";
import { FullScreenPanel } from "@/components/common/FullScreenPanel";
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
import { Switch } from "@/components/ui/switch";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import { Textarea } from "@/components/ui/textarea";
import { ModelDropdown } from "@/components/providers/forms/shared";
import { fetchModelsForConfig, type FetchedModel } from "@/lib/api/model-fetch";
import {
  formatTokenCount,
  resolveCursorModelMetadata,
  type ContextWindowSource,
} from "@/lib/cursorModelMetadata";
import {
  createCursorModelConfig,
  type CursorModelConfig,
  type CursorProvider,
  type CursorProviderType,
} from "@/lib/api/cursor";
import { generateUUID } from "@/utils/uuid";

interface CursorModelDialogProps {
  open: boolean;
  provider: CursorProvider | null;
  onOpenChange: (open: boolean) => void;
  onSave: (provider: CursorProvider) => Promise<void>;
}

type FormState = {
  name: string;
  config: CursorModelConfig;
};

const createFormState = (provider: CursorProvider | null): FormState => ({
  name: provider?.name ?? "",
  config: createCursorModelConfig(provider?.settingsConfig),
});

const parsePositiveInteger = (value: string) => {
  const parsed = Number.parseInt(value, 10);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : 0;
};

const validateJSONObject = (enabled: boolean, value: string, label: string) => {
  if (!enabled) return;
  let parsed: unknown;
  try {
    parsed = JSON.parse(value || "{}");
  } catch {
    throw new Error(`${label}不是有效 JSON`);
  }
  if (!parsed || Array.isArray(parsed) || typeof parsed !== "object") {
    throw new Error(`${label}必须是 JSON 对象`);
  }
};

export function CursorModelDialog({
  open,
  provider,
  onOpenChange,
  onSave,
}: CursorModelDialogProps) {
  const [form, setForm] = useState<FormState>(() => createFormState(provider));
  const [error, setError] = useState("");
  const [saving, setSaving] = useState(false);
  const [discovering, setDiscovering] = useState(false);
  const [discoveredModels, setDiscoveredModels] = useState<FetchedModel[]>([]);
  const [contextWindowSource, setContextWindowSource] =
    useState<ContextWindowSource>("unknown");
  const [advancedOpen, setAdvancedOpen] = useState(false);

  useEffect(() => {
    if (open) {
      setForm(createFormState(provider));
      setError("");
      setDiscoveredModels([]);
      setContextWindowSource("unknown");
      setAdvancedOpen(false);
    }
  }, [open, provider]);

  const setConfig = <K extends keyof CursorModelConfig>(
    key: K,
    value: CursorModelConfig[K],
  ) => {
    setForm((current) => ({
      ...current,
      config: { ...current.config, [key]: value },
    }));
  };

  const handleTypeChange = (type: CursorProviderType) => {
    setDiscoveredModels([]);
    setContextWindowSource("unknown");
    setForm((current) => ({
      ...current,
      config: {
        ...current.config,
        type,
        baseURL:
          current.config.baseURL &&
          current.config.baseURL !== "https://api.openai.com"
            ? current.config.baseURL
            : type === "anthropic"
              ? "https://api.anthropic.com"
              : "https://api.openai.com",
      },
    }));
  };

  const handleDiscoverModels = async () => {
    const baseUrl = form.config.baseURL.trim();
    const apiKey = form.config.apiKey.trim();
    if (!baseUrl || !apiKey) {
      setError("请先填写 Base URL 和 API Key");
      return;
    }

    setDiscovering(true);
    setError("");
    try {
      const models = await fetchModelsForConfig(
        baseUrl,
        apiKey,
        undefined,
        undefined,
        undefined,
        form.config.type,
      );
      setDiscoveredModels(models);
      if (models.length === 0) {
        setError("提供商返回了空模型列表");
      }
    } catch (discoveryError) {
      setDiscoveredModels([]);
      setError(
        `自动发现失败：${
          discoveryError instanceof Error
            ? discoveryError.message
            : String(discoveryError)
        }`,
      );
    } finally {
      setDiscovering(false);
    }
  };

  const handleDiscoveredModelSelect = (modelId: string) => {
    const model = discoveredModels.find(
      (candidate) => candidate.id === modelId,
    );
    if (!model) return;

    const metadata = resolveCursorModelMetadata(
      model,
      form.config.baseURL,
      form.config.type,
    );
    setContextWindowSource(metadata.contextWindowSource);
    setForm((current) => ({
      ...current,
      name: current.name.trim() ? current.name : model.id,
      config: {
        ...current.config,
        modelID: model.id,
        providerGroup:
          current.config.providerGroup.trim() || metadata.providerGroup,
        contextWindowTokens:
          metadata.contextWindowTokens || current.config.contextWindowTokens,
      },
    }));
  };

  const handleSave = async () => {
    const name = form.name.trim();
    const config = {
      ...form.config,
      providerGroup: form.config.providerGroup.trim(),
      baseURL: form.config.baseURL.trim(),
      apiKey: form.config.apiKey.trim(),
      modelID: form.config.modelID.trim(),
      pricingModel: form.config.pricingModel.trim(),
      tooltipData: form.config.tooltipData.trim() || "Managed by CC Switch",
    };
    try {
      if (!name || !config.baseURL || !config.apiKey || !config.modelID) {
        throw new Error("名称、Base URL、API Key 和上游模型不能为空");
      }
      validateJSONObject(
        config.openAIExtraParamsEnabled,
        config.openAIExtraParamsJSON,
        "OpenAI 额外参数",
      );
      validateJSONObject(
        config.anthropicExtraParamsEnabled,
        config.anthropicExtraParamsJSON,
        "Anthropic 额外参数",
      );
      validateJSONObject(
        config.customHeadersEnabled,
        config.customHeadersJSON,
        "自定义请求头",
      );
      setSaving(true);
      await onSave({
        ...(provider ?? {
          id: generateUUID(),
          createdAt: Date.now(),
        }),
        name,
        settingsConfig: config,
        icon: config.type === "anthropic" ? "anthropic" : "openai",
      });
      onOpenChange(false);
    } catch (saveError) {
      setError(
        saveError instanceof Error ? saveError.message : String(saveError),
      );
    } finally {
      setSaving(false);
    }
  };

  const config = form.config;
  const footer = (
    <>
      <span className="mr-auto min-w-0 truncate text-xs text-muted-foreground">
        配置保存后，模型会归入对应的提供商分组。
      </span>
      <Button
        variant="outline"
        onClick={() => onOpenChange(false)}
        disabled={saving}
      >
        取消
      </Button>
      <Button onClick={() => void handleSave()} disabled={saving}>
        {!provider && <Plus className="mr-2 h-4 w-4" />}
        {saving ? "保存中…" : provider ? "保存修改" : "添加模型"}
      </Button>
    </>
  );

  return (
    <FullScreenPanel
      isOpen={open}
      title={provider ? "编辑 Cursor 模型" : "添加 Cursor 模型"}
      onClose={() => onOpenChange(false)}
      footer={footer}
      contentClassName="pt-3"
    >
      <div className="mx-auto w-full max-w-4xl">
        <div className="glass space-y-6 rounded-xl border border-white/10 p-6">
          {error && (
            <div className="rounded-md border border-destructive/30 bg-destructive/5 px-3 py-2 text-sm text-destructive">
              {error}
            </div>
          )}

          <section className="space-y-5">
            <div>
              <h3 className="text-base font-semibold">基础配置</h3>
              <p className="mt-1 text-sm text-muted-foreground">
                填写提供商连接信息，并选择要显示在 Cursor 中的模型。
              </p>
            </div>

            <div className="grid gap-4 sm:grid-cols-2">
              <Field
                label="提供商名称"
                hint="同一提供商名称和接口地址下的模型会归为一组"
              >
                <Input
                  value={config.providerGroup}
                  onChange={(event) =>
                    setConfig("providerGroup", event.target.value)
                  }
                  placeholder="例如 OpenRouter"
                />
              </Field>
              <Field label="API 协议">
                <Select
                  value={config.type}
                  onValueChange={(value) =>
                    handleTypeChange(value as CursorProviderType)
                  }
                >
                  <SelectTrigger>
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="openai">OpenAI Compatible</SelectItem>
                    <SelectItem value="anthropic">
                      Anthropic Compatible
                    </SelectItem>
                  </SelectContent>
                </Select>
              </Field>
            </div>

            <Field label="API 端点" hint="供应商的 API Base URL">
              <Input
                value={config.baseURL}
                onChange={(event) => setConfig("baseURL", event.target.value)}
                placeholder="https://api.example.com"
              />
            </Field>

            <Field label="API Key" hint="凭证仅存储在本机数据库中">
              <Input
                type="password"
                value={config.apiKey}
                onChange={(event) => setConfig("apiKey", event.target.value)}
                autoComplete="new-password"
                placeholder="sk-..."
              />
            </Field>

            <div className="grid gap-4 sm:grid-cols-2">
              <Field label="模型显示名称">
                <Input
                  value={form.name}
                  onChange={(event) =>
                    setForm((current) => ({
                      ...current,
                      name: event.target.value,
                    }))
                  }
                  placeholder="例如 GPT-5 Coding"
                />
              </Field>
              <Field label="上游模型 ID">
                <div className="flex gap-2">
                  <Input
                    value={config.modelID}
                    onChange={(event) =>
                      setConfig("modelID", event.target.value)
                    }
                    placeholder={
                      config.type === "anthropic"
                        ? "claude-sonnet-4-6"
                        : "gpt-5.4"
                    }
                  />
                  {discoveredModels.length > 0 && (
                    <ModelDropdown
                      models={discoveredModels}
                      onSelect={handleDiscoveredModelSelect}
                    />
                  )}
                  <Button
                    type="button"
                    variant="outline"
                    className="shrink-0"
                    disabled={discovering}
                    onClick={() => void handleDiscoverModels()}
                  >
                    <RefreshCw
                      className={`mr-2 h-4 w-4 ${discovering ? "animate-spin" : ""}`}
                    />
                    {discovering
                      ? "获取中…"
                      : discoveredModels.length > 0
                        ? `${discoveredModels.length} 个模型`
                        : "获取模型"}
                  </Button>
                </div>
              </Field>
            </div>
          </section>

          <Collapsible open={advancedOpen} onOpenChange={setAdvancedOpen}>
            <CollapsibleTrigger asChild>
              <Button
                type="button"
                variant={null}
                size="sm"
                className="h-8 gap-1.5 px-0 text-sm font-medium text-foreground hover:opacity-70"
              >
                {advancedOpen ? (
                  <ChevronDown className="h-4 w-4" />
                ) : (
                  <ChevronRight className="h-4 w-4" />
                )}
                高级设置
              </Button>
            </CollapsibleTrigger>
            {!advancedOpen && (
              <p className="ml-1 mt-1 text-xs text-muted-foreground">
                计价、推理参数、Token 限制以及自定义请求参数
              </p>
            )}
            <CollapsibleContent className="space-y-5 pt-4">
              <Field label="计价模型（可选）" hint="留空时使用上游模型 ID">
                <Input
                  value={config.pricingModel}
                  onChange={(event) =>
                    setConfig("pricingModel", event.target.value)
                  }
                  placeholder={config.modelID || "与定价表模型一致"}
                />
              </Field>

              {config.type === "openai" ? (
                <div className="grid gap-4 sm:grid-cols-2">
                  <Field label="OpenAI Endpoint">
                    <Select
                      value={config.openAIEndpoint}
                      onValueChange={(value) =>
                        setConfig("openAIEndpoint", value)
                      }
                    >
                      <SelectTrigger>
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="/v1/responses">
                          Responses API
                        </SelectItem>
                        <SelectItem value="/v1/chat/completions">
                          Chat Completions
                        </SelectItem>
                        <SelectItem value="/custom">Custom</SelectItem>
                      </SelectContent>
                    </Select>
                  </Field>
                  <EffortField
                    label="推理强度"
                    value={config.reasoningEffort}
                    onValueChange={(value) =>
                      setConfig("reasoningEffort", value)
                    }
                  />
                </div>
              ) : (
                <div className="grid gap-4 sm:grid-cols-2">
                  <EffortField
                    label="Thinking Effort"
                    value={config.anthropicThinkingEffort}
                    onValueChange={(value) =>
                      setConfig("anthropicThinkingEffort", value)
                    }
                  />
                  <Field label="Thinking Budget Tokens">
                    <Input
                      inputMode="numeric"
                      value={config.thinkingBudgetTokens || ""}
                      onChange={(event) =>
                        setConfig(
                          "thinkingBudgetTokens",
                          parsePositiveInteger(event.target.value),
                        )
                      }
                      placeholder="4096"
                    />
                  </Field>
                </div>
              )}

              <div className="grid gap-4 sm:grid-cols-3">
                <Field
                  label="上下文窗口"
                  hint={
                    contextWindowSource === "provider"
                      ? `提供商返回 ${formatTokenCount(config.contextWindowTokens)} tokens`
                      : contextWindowSource === "inferred"
                        ? `已推断为 ${formatTokenCount(config.contextWindowTokens)} tokens`
                        : "自动发现未提供时可手工填写"
                  }
                >
                  <Input
                    inputMode="numeric"
                    value={config.contextWindowTokens || ""}
                    onChange={(event) => {
                      setContextWindowSource("unknown");
                      setConfig(
                        "contextWindowTokens",
                        parsePositiveInteger(event.target.value),
                      );
                    }}
                    placeholder="200000"
                  />
                </Field>
                <Field label="最大输出 Tokens">
                  <Input
                    inputMode="numeric"
                    value={config.maxCompletionTokens || ""}
                    onChange={(event) =>
                      setConfig(
                        "maxCompletionTokens",
                        parsePositiveInteger(event.target.value),
                      )
                    }
                    placeholder="65536"
                  />
                </Field>
                <Field label="Anthropic Max Tokens">
                  <Input
                    inputMode="numeric"
                    value={config.anthropicMaxTokens || ""}
                    onChange={(event) =>
                      setConfig(
                        "anthropicMaxTokens",
                        parsePositiveInteger(event.target.value),
                      )
                    }
                    placeholder="65536"
                  />
                </Field>
              </div>

              <JSONOption
                label="自定义请求头"
                enabled={config.customHeadersEnabled}
                value={config.customHeadersJSON}
                onEnabledChange={(value) =>
                  setConfig("customHeadersEnabled", value)
                }
                onValueChange={(value) => setConfig("customHeadersJSON", value)}
              />
              <JSONOption
                label={
                  config.type === "openai"
                    ? "OpenAI 额外参数"
                    : "Anthropic 额外参数"
                }
                enabled={
                  config.type === "openai"
                    ? config.openAIExtraParamsEnabled
                    : config.anthropicExtraParamsEnabled
                }
                value={
                  config.type === "openai"
                    ? config.openAIExtraParamsJSON
                    : config.anthropicExtraParamsJSON
                }
                onEnabledChange={(value) =>
                  setConfig(
                    config.type === "openai"
                      ? "openAIExtraParamsEnabled"
                      : "anthropicExtraParamsEnabled",
                    value,
                  )
                }
                onValueChange={(value) =>
                  setConfig(
                    config.type === "openai"
                      ? "openAIExtraParamsJSON"
                      : "anthropicExtraParamsJSON",
                    value,
                  )
                }
              />
            </CollapsibleContent>
          </Collapsible>
        </div>
      </div>
    </FullScreenPanel>
  );
}

function Field({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: React.ReactNode;
}) {
  return (
    <div className="space-y-1.5">
      <Label>{label}</Label>
      {children}
      {hint && <p className="text-xs text-muted-foreground">{hint}</p>}
    </div>
  );
}

function EffortField({
  label,
  value,
  onValueChange,
}: {
  label: string;
  value: string;
  onValueChange: (value: string) => void;
}) {
  return (
    <Field label={label}>
      <Select value={value} onValueChange={onValueChange}>
        <SelectTrigger>
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {["low", "medium", "high", "xhigh", "max"].map((option) => (
            <SelectItem key={option} value={option}>
              {option}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </Field>
  );
}

function JSONOption({
  label,
  enabled,
  value,
  onEnabledChange,
  onValueChange,
}: {
  label: string;
  enabled: boolean;
  value: string;
  onEnabledChange: (enabled: boolean) => void;
  onValueChange: (value: string) => void;
}) {
  return (
    <div className="rounded-lg border border-border-default p-4">
      <div className="flex items-center justify-between gap-3">
        <Label>{label}</Label>
        <Switch checked={enabled} onCheckedChange={onEnabledChange} />
      </div>
      {enabled && (
        <Textarea
          className="mt-3 min-h-24 font-mono text-xs"
          value={value}
          onChange={(event) => onValueChange(event.target.value)}
          placeholder='{"header": "value"}'
        />
      )}
    </div>
  );
}
