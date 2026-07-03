import {
  piProviderPresets,
  piVendorPresets,
  type PiVendorPreset,
} from "@/config/piProviderPresets";
import type {
  PiProviderDraft,
  PiProviderTemplate,
  PiApiKeyMode,
  PiModelDraft,
  PiApiType,
} from "@/types/pi";
import { useState } from "react";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { Button } from "@/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { Label } from "@/components/ui/label";
import { cn } from "@/lib/utils";
import { Eye, EyeOff, Plus, Trash2 } from "lucide-react";

interface PiProviderFormProps {
  value?: PiProviderDraft;
  onChange: (value: PiProviderDraft) => void;
}

export const emptyPiProviderDraft: PiProviderDraft = {
  mode: "custom",
  providerId: "",
  template: "openAiCompatible",
  baseUrl: "",
  api: "openai-completions",
  apiKey: { mode: "env", value: "" },
  headers: [],
  models: [{ id: "", name: "", nameTouched: false }],
  compat: null,
  advancedJson: null,
};

const API_TYPE_OPTIONS: { value: PiApiType; label: string }[] = [
  { value: "openai-completions", label: "OpenAI Chat Completions" },
  { value: "openai-responses", label: "OpenAI Responses" },
  { value: "anthropic-messages", label: "Anthropic Messages" },
  { value: "google-generative-ai", label: "Google Generative AI" },
];

const API_KEY_MODE_OPTIONS: {
  value: PiApiKeyMode;
  label: string;
  hint: string;
}[] = [
  { value: "env", label: "Environment Variable", hint: "$ENV_VAR_NAME" },
  { value: "literal", label: "Literal Key", hint: "sk-..." },
  {
    value: "command",
    label: "Shell Command",
    hint: "!security find-generic-password ...",
  },
  { value: "none", label: "None (use auth.json / /login)", hint: "" },
];

/** Masked API Key input with Eye/EyeOff toggle — consistent with cc-switch's ApiKeyInput */
function PiApiKeyInput({
  value,
  placeholder,
  onChange,
}: {
  value: string;
  placeholder: string;
  onChange: (val: string) => void;
}) {
  const [showKey, setShowKey] = useState(false);
  return (
    <div className="relative flex-1">
      <input
        type={showKey ? "text" : "password"}
        value={value}
        placeholder={placeholder}
        onChange={(e) => onChange(e.target.value)}
        autoComplete="off"
        className="w-full px-3 py-2 pr-10 border rounded-lg text-sm transition-colors border-border bg-background text-foreground focus:outline-none focus:ring-2 focus:ring-blue-500/20 dark:focus:ring-blue-400/20"
      />
      {value && (
        <button
          type="button"
          onClick={() => setShowKey((v) => !v)}
          className="absolute inset-y-0 right-0 flex items-center pr-3 text-muted-foreground hover:text-foreground transition-colors"
          aria-label={showKey ? "Hide" : "Show"}
        >
          {showKey ? <EyeOff size={16} /> : <Eye size={16} />}
        </button>
      )}
    </div>
  );
}

export function PiProviderForm({ value, onChange }: PiProviderFormProps) {
  const draft = value ?? emptyPiProviderDraft;

  // ─── Template selection ───────────────────────────────────────────────────
  const selectTemplate = (template: PiProviderTemplate) => {
    const preset = piProviderPresets.find((item) => item.id === template);
    onChange({
      ...draft,
      template,
      api: preset?.defaultApi ?? draft.api,
      baseUrl: preset?.defaultBaseUrl ?? draft.baseUrl,
    });
  };

  // ─── Vendor preset (quick fill) ──────────────────────────────────────────
  const applyVendorPreset = (vendor: PiVendorPreset) => {
    onChange({
      ...draft,
      mode: vendor.isBuiltin ? "builtinOverride" : "custom",
      providerId: vendor.providerId,
      template:
        vendor.api === "anthropic-messages"
          ? "anthropicCompatible"
          : vendor.api === "google-generative-ai"
            ? "googleGenerativeAi"
            : vendor.api === "openai-responses"
              ? "openAiResponses"
              : "openAiCompatible",
      baseUrl: vendor.baseUrl ?? "",
      api: vendor.api,
      apiKey: vendor.apiKeyEnvVar
        ? { mode: "env", value: vendor.apiKeyEnvVar }
        : { mode: "none", value: "" },
      models:
        vendor.defaultModels.length > 0
          ? vendor.defaultModels
          : [{ id: "", name: "", nameTouched: false }],
      compat:
        vendor.category === "local"
          ? { supportsDeveloperRole: false, supportsReasoningEffort: false }
          : null,
      headers: [],
      advancedJson: null,
    });
  };

  // ─── Model management ─────────────────────────────────────────────────────
  const updateModel = (index: number, updates: Partial<PiModelDraft>) => {
    const newModels = [...draft.models];
    newModels[index] = { ...newModels[index], ...updates };
    // Auto-fill name from id if not manually touched
    if (updates.id !== undefined && !newModels[index].nameTouched) {
      newModels[index].name = updates.id;
    }
    onChange({ ...draft, models: newModels });
  };

  const addModel = () => {
    onChange({
      ...draft,
      models: [...draft.models, { id: "", name: "", nameTouched: false }],
    });
  };

  const removeModel = (index: number) => {
    if (draft.models.length <= 1) return;
    onChange({ ...draft, models: draft.models.filter((_, i) => i !== index) });
  };

  // ─── Headers ──────────────────────────────────────────────────────────────
  const updateHeadersJson = (raw: string) => {
    try {
      const parsed = raw.trim() ? JSON.parse(raw) : {};
      const headers = Object.entries(parsed).map(([key, headerValue]) => ({
        key,
        value: String(headerValue ?? ""),
      }));
      onChange({ ...draft, headers });
    } catch {
      // Keep invalid JSON until user fixes it
    }
  };

  // ─── Render ───────────────────────────────────────────────────────────────
  return (
    <div className="space-y-6">
      {/* Vendor Quick Select */}
      <section aria-label="Vendor presets" className="space-y-3">
        <h3 className="text-sm font-semibold">Quick Start — Select Provider</h3>
        <div className="grid gap-2 grid-cols-2 md:grid-cols-3 xl:grid-cols-4">
          {piVendorPresets.map((vendor) => (
            <button
              key={vendor.providerId}
              type="button"
              onClick={() => applyVendorPreset(vendor)}
              className={cn(
                "rounded-lg border border-border p-2 text-left transition-colors hover:bg-muted/60",
                draft.providerId === vendor.providerId &&
                  "border-blue-500 bg-blue-500/10",
              )}
            >
              <div className="text-sm font-medium truncate">{vendor.name}</div>
              <div className="mt-0.5 text-[10px] text-muted-foreground truncate">
                {vendor.description}
              </div>
            </button>
          ))}
        </div>
      </section>

      {/* API Template */}
      <section aria-label="API template" className="space-y-3">
        <h3 className="text-sm font-semibold">API Template</h3>
        <div className="grid gap-2 md:grid-cols-3 xl:grid-cols-6">
          {piProviderPresets.map((preset) => (
            <button
              key={preset.id}
              type="button"
              onClick={() => selectTemplate(preset.id)}
              className={cn(
                "rounded-lg border border-border p-2 text-left transition-colors hover:bg-muted/60",
                draft.template === preset.id &&
                  "border-blue-500 bg-blue-500/10",
              )}
            >
              <div className="text-xs font-medium">{preset.label}</div>
            </button>
          ))}
        </div>
      </section>

      {/* Provider Configuration */}
      <section aria-label="Provider config" className="space-y-4">
        <h3 className="text-sm font-semibold">Provider Configuration</h3>
        <div className="grid gap-3 md:grid-cols-2">
          <label className="space-y-1">
            <span className="text-sm font-medium">Provider ID</span>
            <Input
              aria-label="Provider ID"
              value={draft.providerId}
              placeholder="my-openai"
              onChange={(e) =>
                onChange({ ...draft, providerId: e.target.value })
              }
            />
            <span className="text-[10px] text-muted-foreground">
              Key name in models.json "providers" object
            </span>
          </label>
          <label className="space-y-1">
            <span className="text-sm font-medium">Base URL</span>
            <Input
              aria-label="Base URL"
              value={draft.baseUrl ?? ""}
              placeholder="https://api.example.com/v1"
              onChange={(e) => onChange({ ...draft, baseUrl: e.target.value })}
            />
          </label>
          <label className="space-y-1">
            <span className="text-sm font-medium">API Type</span>
            <Select
              value={draft.api}
              onValueChange={(v) => onChange({ ...draft, api: v })}
            >
              <SelectTrigger>
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {API_TYPE_OPTIONS.map((opt) => (
                  <SelectItem key={opt.value} value={opt.value}>
                    {opt.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </label>
          <div className="space-y-1">
            <span className="text-sm font-medium">API Key</span>
            <div className="flex gap-2">
              <Select
                value={draft.apiKey.mode}
                onValueChange={(v) =>
                  onChange({
                    ...draft,
                    apiKey: {
                      mode: v as PiApiKeyMode,
                      value: draft.apiKey.value,
                    },
                  })
                }
              >
                <SelectTrigger className="w-[160px]">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {API_KEY_MODE_OPTIONS.map((opt) => (
                    <SelectItem key={opt.value} value={opt.value}>
                      {opt.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              {draft.apiKey.mode !== "none" && (
                <PiApiKeyInput
                  value={draft.apiKey.value}
                  placeholder={
                    API_KEY_MODE_OPTIONS.find(
                      (o) => o.value === draft.apiKey.mode,
                    )?.hint ?? ""
                  }
                  onChange={(val) =>
                    onChange({
                      ...draft,
                      apiKey: { ...draft.apiKey, value: val },
                    })
                  }
                />
              )}
            </div>
            <span className="text-[10px] text-muted-foreground">
              Pi resolves $VAR from env, !cmd from shell, or literal value
            </span>
          </div>
        </div>
      </section>

      {/* Models */}
      <section aria-label="Models" className="space-y-3">
        <div className="flex items-center justify-between">
          <h3 className="text-sm font-semibold">Models</h3>
          <Button type="button" variant="outline" size="sm" onClick={addModel}>
            <Plus className="mr-1 h-3 w-3" />
            Add Model
          </Button>
        </div>
        <div className="space-y-3">
          {draft.models.map((model, idx) => (
            <div
              key={idx}
              className="grid gap-2 rounded-lg border border-border p-3 md:grid-cols-[1fr_1fr_auto_auto_auto]"
            >
              <Input
                aria-label="Model ID"
                placeholder="model-id"
                value={model.id}
                onChange={(e) => updateModel(idx, { id: e.target.value })}
              />
              <Input
                aria-label="Model Name"
                placeholder="Display name"
                value={model.name ?? ""}
                onChange={(e) =>
                  updateModel(idx, { name: e.target.value, nameTouched: true })
                }
              />
              <div className="flex items-center gap-1">
                <Switch
                  checked={model.reasoning ?? false}
                  onCheckedChange={(v) => updateModel(idx, { reasoning: v })}
                />
                <Label className="text-xs">Reasoning</Label>
              </div>
              <Input
                aria-label="Context window"
                placeholder="ctx"
                type="number"
                className="w-20"
                value={model.contextWindow ?? ""}
                onChange={(e) =>
                  updateModel(idx, {
                    contextWindow: e.target.value
                      ? Number(e.target.value)
                      : undefined,
                  })
                }
              />
              <Button
                type="button"
                variant="ghost"
                size="icon"
                onClick={() => removeModel(idx)}
                disabled={draft.models.length <= 1}
              >
                <Trash2 className="h-4 w-4 text-destructive" />
              </Button>
            </div>
          ))}
        </div>
      </section>

      {/* Compat (for local/partial compatibility) */}
      {(draft.api === "openai-completions" ||
        draft.api === "anthropic-messages") && (
        <section aria-label="Compatibility" className="space-y-3">
          <h3 className="text-sm font-semibold">Compatibility Flags</h3>
          <div className="flex flex-wrap gap-4">
            {draft.api === "openai-completions" && (
              <>
                <div className="flex items-center gap-2">
                  <Switch
                    checked={draft.compat?.supportsDeveloperRole !== false}
                    onCheckedChange={(v) =>
                      onChange({
                        ...draft,
                        compat: { ...draft.compat, supportsDeveloperRole: v },
                      })
                    }
                  />
                  <Label className="text-xs">developer role</Label>
                </div>
                <div className="flex items-center gap-2">
                  <Switch
                    checked={draft.compat?.supportsReasoningEffort !== false}
                    onCheckedChange={(v) =>
                      onChange({
                        ...draft,
                        compat: { ...draft.compat, supportsReasoningEffort: v },
                      })
                    }
                  />
                  <Label className="text-xs">reasoning_effort</Label>
                </div>
                <div className="flex items-center gap-2">
                  <Switch
                    checked={draft.compat?.supportsUsageInStreaming !== false}
                    onCheckedChange={(v) =>
                      onChange({
                        ...draft,
                        compat: {
                          ...draft.compat,
                          supportsUsageInStreaming: v,
                        },
                      })
                    }
                  />
                  <Label className="text-xs">usage in streaming</Label>
                </div>
              </>
            )}
            {draft.api === "anthropic-messages" && (
              <>
                <div className="flex items-center gap-2">
                  <Switch
                    checked={
                      draft.compat?.supportsEagerToolInputStreaming !== false
                    }
                    onCheckedChange={(v) =>
                      onChange({
                        ...draft,
                        compat: {
                          ...draft.compat,
                          supportsEagerToolInputStreaming: v,
                        },
                      })
                    }
                  />
                  <Label className="text-xs">eager tool streaming</Label>
                </div>
                <div className="flex items-center gap-2">
                  <Switch
                    checked={draft.compat?.forceAdaptiveThinking ?? false}
                    onCheckedChange={(v) =>
                      onChange({
                        ...draft,
                        compat: { ...draft.compat, forceAdaptiveThinking: v },
                      })
                    }
                  />
                  <Label className="text-xs">adaptive thinking</Label>
                </div>
              </>
            )}
          </div>
        </section>
      )}

      {/* Advanced (headers + raw JSON) */}
      <section aria-label="Advanced config" className="space-y-3">
        <h3 className="text-sm font-semibold">Advanced</h3>
        <label className="space-y-1">
          <span className="text-xs text-muted-foreground">
            Custom Headers (JSON object, supports $ENV_VAR and !command)
          </span>
          <Textarea
            aria-label="Headers JSON"
            placeholder='{"x-extra":"$EXTRA_TOKEN"}'
            defaultValue={
              draft.headers.length > 0
                ? JSON.stringify(
                    Object.fromEntries(
                      draft.headers.map((h) => [h.key, h.value]),
                    ),
                    null,
                    2,
                  )
                : ""
            }
            onBlur={(e) => updateHeadersJson(e.target.value)}
            rows={3}
          />
        </label>
        <div className="flex gap-2">
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={() => onChange({ ...emptyPiProviderDraft })}
          >
            Reset All
          </Button>
        </div>
      </section>
    </div>
  );
}
