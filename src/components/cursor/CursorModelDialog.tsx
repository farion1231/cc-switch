import { useEffect, useState } from "react";
import { RefreshCw } from "lucide-react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
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

  useEffect(() => {
    if (open) {
      setForm(createFormState(provider));
      setError("");
      setDiscoveredModels([]);
      setContextWindowSource("unknown");
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
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-3xl">
        <DialogHeader>
          <DialogTitle>
            {provider ? "编辑 Cursor 模型" : "添加 Cursor 模型"}
          </DialogTitle>
          <DialogDescription>
            一个配置对应 Cursor 模型选择器中的一个模型。Provider ID
            创建后保持不变。
          </DialogDescription>
        </DialogHeader>
        <div className="grid gap-5 overflow-y-auto px-6 py-5">
          {error && (
            <div className="rounded-md border border-red-500/30 bg-red-500/5 px-3 py-2 text-sm text-red-600 dark:text-red-400">
              {error}
            </div>
          )}
          <div className="grid gap-4 sm:grid-cols-2">
            <Field label="显示名称">
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
            <Field label="协议类型">
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
          <Field
            label="Endpoint 名称"
            hint="同一 Base URL 下的模型归为同一组；该名称用于组标题"
          >
            <Input
              value={config.providerGroup}
              onChange={(event) =>
                setConfig("providerGroup", event.target.value)
              }
              placeholder="例如 OpenRouter 主线路"
            />
          </Field>
          <Field label="Base URL">
            <Input
              value={config.baseURL}
              onChange={(event) => setConfig("baseURL", event.target.value)}
              placeholder="https://api.example.com"
            />
          </Field>
          <Field label="API Key">
            <Input
              type="password"
              value={config.apiKey}
              onChange={(event) => setConfig("apiKey", event.target.value)}
              autoComplete="new-password"
              placeholder="仅存储在本机数据库"
            />
          </Field>
          <div className="grid gap-4 sm:grid-cols-2">
            <Field label="实际上游模型">
              <div className="flex gap-2">
                <Input
                  value={config.modelID}
                  onChange={(event) => setConfig("modelID", event.target.value)}
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
                    ? "发现中…"
                    : discoveredModels.length > 0
                      ? `已发现 ${discoveredModels.length}`
                      : "自动发现"}
                </Button>
              </div>
            </Field>
            <Field label="计价模型（可选）" hint="留空时使用实际上游模型">
              <Input
                value={config.pricingModel}
                onChange={(event) =>
                  setConfig("pricingModel", event.target.value)
                }
                placeholder={config.modelID || "与定价表模型一致"}
              />
            </Field>
          </div>
          {config.type === "openai" ? (
            <div className="grid gap-4 sm:grid-cols-2">
              <Field label="OpenAI endpoint">
                <Select
                  value={config.openAIEndpoint}
                  onValueChange={(value) => setConfig("openAIEndpoint", value)}
                >
                  <SelectTrigger>
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="/v1/responses">Responses API</SelectItem>
                    <SelectItem value="/v1/chat/completions">
                      Chat Completions
                    </SelectItem>
                    <SelectItem value="/custom">Custom</SelectItem>
                  </SelectContent>
                </Select>
              </Field>
              <Field label="推理强度">
                <Select
                  value={config.reasoningEffort}
                  onValueChange={(value) => setConfig("reasoningEffort", value)}
                >
                  <SelectTrigger>
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {["low", "medium", "high", "xhigh", "max"].map((value) => (
                      <SelectItem key={value} value={value}>
                        {value}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </Field>
            </div>
          ) : (
            <div className="grid gap-4 sm:grid-cols-2">
              <Field label="Thinking effort">
                <Select
                  value={config.anthropicThinkingEffort}
                  onValueChange={(value) =>
                    setConfig("anthropicThinkingEffort", value)
                  }
                >
                  <SelectTrigger>
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {["low", "medium", "high", "xhigh", "max"].map((value) => (
                      <SelectItem key={value} value={value}>
                        {value}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </Field>
              <Field label="Thinking budget tokens">
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
                  ? `由提供商接口返回（${formatTokenCount(config.contextWindowTokens)} tokens）`
                  : contextWindowSource === "inferred"
                    ? `根据模型名称推断（${formatTokenCount(config.contextWindowTokens)} tokens），可手工覆盖`
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
            <Field label="最大输出 tokens">
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
            <Field label="Anthropic max tokens">
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
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            取消
          </Button>
          <Button onClick={() => void handleSave()} disabled={saving}>
            {saving ? "保存中…" : "保存模型"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
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
