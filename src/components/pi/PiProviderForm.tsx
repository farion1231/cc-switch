import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
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
  PiHeaderDraft,
} from "@/types/pi";
import { piApi } from "@/lib/api";
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
import { Plus, Trash2 } from "lucide-react";
import JsonEditor from "@/components/JsonEditor";
import { useDarkMode } from "@/hooks/useDarkMode";

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

const API_TYPE_OPTIONS: { value: PiApiType; labelKey: string }[] = [
  { value: "openai-completions", labelKey: "pi.form.apiTypeOpenaiCompletions" },
  { value: "openai-responses", labelKey: "pi.form.apiTypeOpenaiResponses" },
  { value: "anthropic-messages", labelKey: "pi.form.apiTypeAnthropicMessages" },
  {
    value: "google-generative-ai",
    labelKey: "pi.form.apiTypeGoogleGenerativeAi",
  },
];

const API_KEY_MODE_OPTIONS: {
  value: PiApiKeyMode;
  labelKey: string;
  hint: string;
}[] = [
  { value: "env", labelKey: "pi.form.apiKeyModeEnv", hint: "$ENV_VAR_NAME" },
  { value: "literal", labelKey: "pi.form.apiKeyModeLiteral", hint: "sk-..." },
  {
    value: "command",
    labelKey: "pi.form.apiKeyModeCommand",
    hint: "!security find-generic-password ...",
  },
  { value: "none", labelKey: "pi.form.apiKeyModeNone", hint: "" },
];

function headersToJson(headers: PiHeaderDraft[]): string {
  return headers.length > 0
    ? JSON.stringify(
        Object.fromEntries(headers.map((h) => [h.key, h.value])),
        null,
        2,
      )
    : "";
}

export function PiProviderForm({ value, onChange }: PiProviderFormProps) {
  const { t } = useTranslation();
  const isDarkMode = useDarkMode();
  const draft = value ?? emptyPiProviderDraft;
  // Config JSON preview is READ-ONLY and backend-driven, so it always shows
  // exactly what `apply_pi_provider_patch` will write to models.json (no
  // divergent client-side serializer).
  const [previewJson, setPreviewJson] = useState("");
  const [previewError, setPreviewError] = useState<string | null>(null);
  const [previewLoading, setPreviewLoading] = useState(false);

  useEffect(() => {
    const providerId = draft.providerId.trim();
    if (!providerId) {
      setPreviewJson("");
      setPreviewError(null);
      setPreviewLoading(false);
      return;
    }
    let cancelled = false;
    setPreviewLoading(true);
    const handle = setTimeout(async () => {
      try {
        const result = await piApi.previewProviderPatch(draft);
        if (cancelled) return;
        const providers = (
          result.nextModelsJson as Record<string, unknown> | undefined
        )?.providers as Record<string, unknown> | undefined;
        const obj = providers?.[providerId] ?? {};
        setPreviewJson(JSON.stringify(obj, null, 2));
        setPreviewError(null);
      } catch (error) {
        if (cancelled) return;
        setPreviewJson("");
        setPreviewError(error instanceof Error ? error.message : String(error));
      } finally {
        if (!cancelled) setPreviewLoading(false);
      }
    }, 300);
    return () => {
      cancelled = true;
      clearTimeout(handle);
    };
  }, [draft]);

  // Headers is a free-form JSON textarea. It is controlled, but we only
  // re-derive its text from draft.headers when the field is NOT focused, so
  // typing isn't interrupted (and Reset/preset changes still propagate).
  const [headersText, setHeadersText] = useState(() =>
    headersToJson(draft.headers),
  );
  const headersFocused = useRef(false);
  useEffect(() => {
    if (headersFocused.current) return;
    setHeadersText((prev) => {
      const derived = headersToJson(draft.headers);
      return prev === derived ? prev : derived;
    });
  }, [draft.headers]);

  const commitHeaders = (raw: string) => {
    try {
      const parsed = raw.trim() ? JSON.parse(raw) : {};
      const headers = Object.entries(parsed).map(([key, headerValue]) => ({
        key,
        value: String(headerValue ?? ""),
      }));
      onChange({ ...draft, headers });
    } catch {
      // Keep invalid JSON until the user fixes it.
    }
  };

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

  // ─── Render ───────────────────────────────────────────────────────────────
  return (
    <div className="space-y-6">
      {/* Vendor Quick Select */}
      <section aria-label="Vendor presets" className="space-y-3">
        <h3 className="text-sm font-semibold">{t("pi.form.quickStart")}</h3>
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
        <h3 className="text-sm font-semibold">{t("pi.form.apiTemplate")}</h3>
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
        <h3 className="text-sm font-semibold">{t("pi.form.providerConfig")}</h3>
        <div className="grid gap-3 md:grid-cols-2">
          <label className="space-y-1">
            <span className="text-sm font-medium">
              {t("pi.form.providerId")}
            </span>
            <Input
              aria-label="Provider ID"
              value={draft.providerId}
              placeholder="my-openai"
              onChange={(e) =>
                onChange({ ...draft, providerId: e.target.value })
              }
            />
            <span className="text-[10px] text-muted-foreground">
              {t("pi.form.providerIdHint")}
            </span>
          </label>
          <label className="space-y-1">
            <span className="text-sm font-medium">{t("pi.form.baseUrl")}</span>
            <Input
              aria-label="Base URL"
              value={draft.baseUrl ?? ""}
              placeholder="https://api.example.com/v1"
              onChange={(e) => onChange({ ...draft, baseUrl: e.target.value })}
            />
          </label>
          <label className="space-y-1">
            <span className="text-sm font-medium">{t("pi.form.apiType")}</span>
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
                    {t(opt.labelKey)}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </label>
          <div className="space-y-1">
            <span className="text-sm font-medium">{t("pi.form.apiKey")}</span>
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
                      {t(opt.labelKey)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              {draft.apiKey.mode !== "none" && (
                <Input
                  aria-label="API Key value"
                  value={draft.apiKey.value}
                  placeholder={
                    API_KEY_MODE_OPTIONS.find(
                      (o) => o.value === draft.apiKey.mode,
                    )?.hint ?? ""
                  }
                  onChange={(e) =>
                    onChange({
                      ...draft,
                      apiKey: { ...draft.apiKey, value: e.target.value },
                    })
                  }
                  className="flex-1"
                />
              )}
            </div>
            <span className="text-[10px] text-muted-foreground">
              {t("pi.form.apiKeyHint")}
            </span>
          </div>
        </div>
      </section>

      {/* Extra Options / Headers */}
      <section aria-label="Extra options" className="space-y-3">
        <h3 className="text-sm font-semibold">{t("pi.form.extraOptions")}</h3>
        <label className="space-y-1">
          <span className="text-xs text-muted-foreground">
            {t("pi.form.headersLabel")}
          </span>
          <Textarea
            aria-label="Headers JSON"
            placeholder='{"x-extra":"$EXTRA_TOKEN"}'
            value={headersText}
            onFocus={() => {
              headersFocused.current = true;
            }}
            onChange={(e) => setHeadersText(e.target.value)}
            onBlur={(e) => {
              headersFocused.current = false;
              commitHeaders(e.target.value);
            }}
            rows={3}
          />
        </label>
      </section>

      {/* Models */}
      <section aria-label="Models" className="space-y-3">
        <div className="flex items-center justify-between">
          <h3 className="text-sm font-semibold">{t("pi.form.models")}</h3>
          <Button type="button" variant="outline" size="sm" onClick={addModel}>
            <Plus className="mr-1 h-3 w-3" />
            {t("pi.form.addModel")}
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
                placeholder={t("pi.form.modelNamePlaceholder")}
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
                <Label className="text-xs">{t("pi.form.reasoning")}</Label>
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
          <h3 className="text-sm font-semibold">{t("pi.form.compatFlags")}</h3>
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
                  <Label className="text-xs">
                    {t("pi.form.developerRole")}
                  </Label>
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
                  <Label className="text-xs">
                    {t("pi.form.reasoningEffort")}
                  </Label>
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
                  <Label className="text-xs">
                    {t("pi.form.usageInStreaming")}
                  </Label>
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
                  <Label className="text-xs">
                    {t("pi.form.eagerToolStreaming")}
                  </Label>
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
                  <Label className="text-xs">
                    {t("pi.form.adaptiveThinking")}
                  </Label>
                </div>
              </>
            )}
          </div>
        </section>
      )}

      {/* Config JSON preview (read-only, backend-driven) */}
      <section aria-label="Config JSON" className="space-y-3">
        <div className="flex items-center justify-between">
          <h3 className="text-sm font-semibold">{t("pi.form.configJson")}</h3>
          <span className="text-xs text-muted-foreground">
            {previewLoading
              ? t("pi.form.configJsonLoading")
              : t("pi.form.configJsonReadonly")}
          </span>
        </div>
        <JsonEditor
          value={previewJson}
          onChange={() => {}}
          readOnly
          rows={14}
          showValidation={false}
          language="json"
          darkMode={isDarkMode}
        />
        {previewError && (
          <p className="text-xs text-destructive">{previewError}</p>
        )}
      </section>

      {/* Actions */}
      <section aria-label="Actions" className="space-y-3">
        <div className="flex gap-2">
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={() => onChange({ ...emptyPiProviderDraft })}
          >
            {t("pi.form.resetAll")}
          </Button>
        </div>
      </section>
    </div>
  );
}
