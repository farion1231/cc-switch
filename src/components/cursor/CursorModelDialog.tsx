import { useEffect, useState } from "react";
import { ChevronDown, ChevronRight, Plus, RefreshCw } from "lucide-react";
import { useTranslation } from "react-i18next";
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

const preserveEndpointConfig = (
  config: CursorModelConfig,
  provider: CursorProvider | null,
): CursorModelConfig => {
  if (!provider) return config;

  const endpointConfig = provider.settingsConfig;
  return {
    ...config,
    providerGroup: endpointConfig.providerGroup,
    endpointId: endpointConfig.endpointId,
    type: endpointConfig.type,
    baseURL: endpointConfig.baseURL,
    apiKey: endpointConfig.apiKey,
  };
};

export function CursorModelDialog({
  open,
  provider,
  onOpenChange,
  onSave,
}: CursorModelDialogProps) {
  const { t } = useTranslation();
  const validateJSONObject = (
    enabled: boolean,
    value: string,
    label: string,
  ) => {
    if (!enabled) return;
    let parsed: unknown;
    try {
      parsed = JSON.parse(value || "{}");
    } catch {
      throw new Error(t("cursor.modelDialog.error.invalidJson", { label }));
    }
    if (!parsed || Array.isArray(parsed) || typeof parsed !== "object") {
      throw new Error(
        t("cursor.modelDialog.error.jsonObjectRequired", { label }),
      );
    }
  };

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
      setError(t("cursor.modelDialog.error.credentialsRequired"));
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
        setError(t("cursor.modelDialog.error.emptyModelList"));
      }
    } catch (discoveryError) {
      setDiscoveredModels([]);
      setError(
        t("cursor.modelDialog.error.discoveryFailed", {
          error:
            discoveryError instanceof Error
              ? discoveryError.message
              : String(discoveryError),
        }),
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
    const config = preserveEndpointConfig(
      {
        ...form.config,
        providerGroup: form.config.providerGroup.trim(),
        baseURL: form.config.baseURL.trim(),
        apiKey: form.config.apiKey.trim(),
        modelID: form.config.modelID.trim(),
        pricingModel: form.config.pricingModel.trim(),
        tooltipData:
          form.config.tooltipData.trim() ||
          t("cursor.modelDialog.defaults.tooltipData"),
      },
      provider,
    );
    try {
      if (!name || !config.baseURL || !config.apiKey || !config.modelID) {
        throw new Error(t("cursor.modelDialog.error.requiredFields"));
      }
      validateJSONObject(
        config.openAIExtraParamsEnabled,
        config.openAIExtraParamsJSON,
        t("cursor.modelDialog.fields.openAIExtraParams"),
      );
      validateJSONObject(
        config.anthropicExtraParamsEnabled,
        config.anthropicExtraParamsJSON,
        t("cursor.modelDialog.fields.anthropicExtraParams"),
      );
      validateJSONObject(
        config.customHeadersEnabled,
        config.customHeadersJSON,
        t("cursor.modelDialog.fields.customHeaders"),
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
        {t("cursor.modelDialog.footerHint")}
      </span>
      <Button
        variant="outline"
        onClick={() => onOpenChange(false)}
        disabled={saving}
      >
        {t("common.cancel")}
      </Button>
      <Button onClick={() => void handleSave()} disabled={saving}>
        {!provider && <Plus className="mr-2 h-4 w-4" />}
        {saving
          ? t("common.saving")
          : provider
            ? t("cursor.modelDialog.saveChanges")
            : t("cursor.modelDialog.addModel")}
      </Button>
    </>
  );

  return (
    <FullScreenPanel
      isOpen={open}
      title={
        provider
          ? t("cursor.modelDialog.editTitle")
          : t("cursor.modelDialog.addTitle")
      }
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
              <h3 className="text-base font-semibold">
                {t("cursor.modelDialog.basic.title")}
              </h3>
              <p className="mt-1 text-sm text-muted-foreground">
                {t("cursor.modelDialog.basic.description")}
              </p>
            </div>

            <div className="grid gap-4 sm:grid-cols-2">
              <Field
                label={t("cursor.modelDialog.fields.providerName")}
                hint={t("cursor.modelDialog.fields.providerNameHint")}
              >
                <Input
                  value={config.providerGroup}
                  disabled={Boolean(provider)}
                  onChange={(event) =>
                    setConfig("providerGroup", event.target.value)
                  }
                  placeholder={t(
                    "cursor.modelDialog.placeholders.providerName",
                  )}
                />
              </Field>
              <Field label={t("cursor.modelDialog.fields.apiProtocol")}>
                <Select
                  value={config.type}
                  disabled={Boolean(provider)}
                  onValueChange={(value) =>
                    handleTypeChange(value as CursorProviderType)
                  }
                >
                  <SelectTrigger>
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="openai">
                      {t("cursor.protocol.openAICompatible")}
                    </SelectItem>
                    <SelectItem value="anthropic">
                      {t("cursor.protocol.anthropicCompatible")}
                    </SelectItem>
                  </SelectContent>
                </Select>
              </Field>
            </div>

            <Field
              label={t("cursor.modelDialog.fields.apiEndpoint")}
              hint={t("cursor.modelDialog.fields.apiEndpointHint")}
            >
              <Input
                value={config.baseURL}
                disabled={Boolean(provider)}
                onChange={(event) => setConfig("baseURL", event.target.value)}
                placeholder={t("cursor.modelDialog.placeholders.apiEndpoint")}
              />
            </Field>

            <Field
              label={t("cursor.modelDialog.fields.apiKey")}
              hint={t("cursor.modelDialog.fields.apiKeyHint")}
            >
              <Input
                type="password"
                value={config.apiKey}
                disabled={Boolean(provider)}
                onChange={(event) => setConfig("apiKey", event.target.value)}
                autoComplete="new-password"
                placeholder={t("cursor.modelDialog.placeholders.apiKey")}
              />
            </Field>

            <div className="grid gap-4 sm:grid-cols-2">
              <Field label={t("cursor.modelDialog.fields.displayName")}>
                <Input
                  value={form.name}
                  onChange={(event) =>
                    setForm((current) => ({
                      ...current,
                      name: event.target.value,
                    }))
                  }
                  placeholder={t("cursor.modelDialog.placeholders.displayName")}
                />
              </Field>
              <Field label={t("cursor.modelDialog.fields.upstreamModelId")}>
                <div className="flex gap-2">
                  <Input
                    value={config.modelID}
                    onChange={(event) =>
                      setConfig("modelID", event.target.value)
                    }
                    placeholder={
                      config.type === "anthropic"
                        ? t("cursor.modelDialog.placeholders.anthropicModelId")
                        : t("cursor.modelDialog.placeholders.openAIModelId")
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
                      ? t("cursor.modelDialog.models.fetching")
                      : discoveredModels.length > 0
                        ? t("cursor.modelDialog.models.count", {
                            count: discoveredModels.length,
                          })
                        : t("cursor.modelDialog.models.fetchAction")}
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
                {t("cursor.modelDialog.advanced.title")}
              </Button>
            </CollapsibleTrigger>
            {!advancedOpen && (
              <p className="ml-1 mt-1 text-xs text-muted-foreground">
                {t("cursor.modelDialog.advanced.description")}
              </p>
            )}
            <CollapsibleContent className="space-y-5 pt-4">
              <Field
                label={t("cursor.modelDialog.fields.pricingModel")}
                hint={t("cursor.modelDialog.fields.pricingModelHint")}
              >
                <Input
                  value={config.pricingModel}
                  onChange={(event) =>
                    setConfig("pricingModel", event.target.value)
                  }
                  placeholder={
                    config.modelID ||
                    t("cursor.modelDialog.placeholders.pricingModel")
                  }
                />
              </Field>

              {config.type === "openai" ? (
                <div className="grid gap-4 sm:grid-cols-2">
                  <Field label={t("cursor.modelDialog.fields.openAIEndpoint")}>
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
                          {t("cursor.modelDialog.options.responsesApi")}
                        </SelectItem>
                        <SelectItem value="/v1/chat/completions">
                          {t("cursor.modelDialog.options.chatCompletions")}
                        </SelectItem>
                        <SelectItem value="/custom">
                          {t("cursor.modelDialog.options.custom")}
                        </SelectItem>
                      </SelectContent>
                    </Select>
                  </Field>
                  <EffortField
                    label={t("cursor.modelDialog.fields.reasoningEffort")}
                    value={config.reasoningEffort}
                    onValueChange={(value) =>
                      setConfig("reasoningEffort", value)
                    }
                  />
                </div>
              ) : (
                <div className="grid gap-4 sm:grid-cols-2">
                  <EffortField
                    label={t("cursor.modelDialog.fields.thinkingEffort")}
                    value={config.anthropicThinkingEffort}
                    onValueChange={(value) =>
                      setConfig("anthropicThinkingEffort", value)
                    }
                  />
                  <Field
                    label={t("cursor.modelDialog.fields.thinkingBudgetTokens")}
                  >
                    <Input
                      inputMode="numeric"
                      value={config.thinkingBudgetTokens || ""}
                      onChange={(event) =>
                        setConfig(
                          "thinkingBudgetTokens",
                          parsePositiveInteger(event.target.value),
                        )
                      }
                      placeholder={t(
                        "cursor.modelDialog.placeholders.thinkingBudgetTokens",
                      )}
                    />
                  </Field>
                </div>
              )}

              <div className="grid gap-4 sm:grid-cols-3">
                <Field
                  label={t("cursor.modelDialog.fields.contextWindow")}
                  hint={
                    contextWindowSource === "provider"
                      ? t("cursor.modelDialog.contextWindow.provider", {
                          tokens: formatTokenCount(config.contextWindowTokens),
                        })
                      : contextWindowSource === "inferred"
                        ? t("cursor.modelDialog.contextWindow.inferred", {
                            tokens: formatTokenCount(
                              config.contextWindowTokens,
                            ),
                          })
                        : t("cursor.modelDialog.contextWindow.manualHint")
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
                    placeholder={t(
                      "cursor.modelDialog.placeholders.contextWindowTokens",
                    )}
                  />
                </Field>
                <Field label={t("cursor.modelDialog.fields.maxOutputTokens")}>
                  <Input
                    inputMode="numeric"
                    value={config.maxCompletionTokens || ""}
                    onChange={(event) =>
                      setConfig(
                        "maxCompletionTokens",
                        parsePositiveInteger(event.target.value),
                      )
                    }
                    placeholder={t(
                      "cursor.modelDialog.placeholders.maxOutputTokens",
                    )}
                  />
                </Field>
                <Field
                  label={t("cursor.modelDialog.fields.anthropicMaxTokens")}
                >
                  <Input
                    inputMode="numeric"
                    value={config.anthropicMaxTokens || ""}
                    onChange={(event) =>
                      setConfig(
                        "anthropicMaxTokens",
                        parsePositiveInteger(event.target.value),
                      )
                    }
                    placeholder={t(
                      "cursor.modelDialog.placeholders.anthropicMaxTokens",
                    )}
                  />
                </Field>
              </div>

              <JSONOption
                label={t("cursor.modelDialog.fields.customHeaders")}
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
                    ? t("cursor.modelDialog.fields.openAIExtraParams")
                    : t("cursor.modelDialog.fields.anthropicExtraParams")
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
  const { t } = useTranslation();

  return (
    <Field label={label}>
      <Select value={value} onValueChange={onValueChange}>
        <SelectTrigger>
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {["low", "medium", "high", "xhigh", "max"].map((option) => (
            <SelectItem key={option} value={option}>
              {t(`cursor.modelDialog.effort.${option}`)}
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
  const { t } = useTranslation();

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
          placeholder={t("cursor.modelDialog.placeholders.jsonObject")}
        />
      )}
    </div>
  );
}
