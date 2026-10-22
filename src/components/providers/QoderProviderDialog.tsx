import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Loader2, Plus, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Checkbox } from "@/components/ui/checkbox";
import { FullScreenPanel } from "@/components/common/FullScreenPanel";
import { APP_DISPLAY_NAME } from "@/components/shell/AppGlyph";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import type { QoderAppId, QoderModel, QoderProvider } from "@/lib/api/qoder";
import { extractErrorMessage } from "@/utils/errorUtils";
import {
  qoderProviderPresets,
  qoderAccountPreset,
  QODER_ACCOUNT_PRESET_ID,
} from "@/config/qoderProviderPresets";
import { QoderAccountLogin } from "./QoderAccountLogin";
import { ProviderPresetSelector } from "./forms/ProviderPresetSelector";
import { PresetStepContext, type PresetStepState } from "./forms/presetStep";

const PRESETS = qoderProviderPresets.map((preset) => ({
  id: preset.providerKey,
  preset,
}));

const newModel = (): QoderModel => ({
  model: "",
  capabilities: { tools: true, vision: false },
});

function formConfig(provider?: QoderProvider): QoderProvider {
  const initial: QoderProvider = provider ?? {
    type: "openai-compatible",
    protocol: "openai",
    authType: "bearer",
    baseUrl: "",
    apiKey: "",
  };
  const models = initial.models?.length
    ? [...initial.models]
    : initial.model
      ? [{ model: initial.model }]
      : [newModel()];
  if (initial.model && !models.some((model) => model.model === initial.model))
    models.push({ model: initial.model });
  return {
    ...initial,
    baseUrl: typeof initial.baseUrl === "string" ? initial.baseUrl : "",
    apiKey: typeof initial.apiKey === "string" ? initial.apiKey : "",
    models: structuredClone(models),
  };
}

export function QoderProviderDialog({
  app,
  id: originalId,
  provider,
  providers,
  currentModel,
  accountLogin = false,
  onUseAccount,
  onSave,
  onClose,
}: {
  app: QoderAppId;
  id?: string;
  provider?: QoderProvider;
  providers: Record<string, QoderProvider>;
  currentModel: string;
  accountLogin?: boolean;
  onUseAccount: () => Promise<void>;
  onSave: (
    id: string,
    provider: QoderProvider,
    makeDefault: boolean,
  ) => Promise<void>;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const [step, setStep] = useState<"pick" | "form">(
    originalId || accountLogin ? "form" : "pick",
  );
  const [pickerHost, setPickerHost] = useState<HTMLDivElement | null>(null);
  const [accountActionHost, setAccountActionHost] =
    useState<HTMLDivElement | null>(null);
  const [presetId, setPresetId] = useState(
    accountLogin ? QODER_ACCOUNT_PRESET_ID : "custom",
  );
  const isAccountLogin = !originalId && presetId === QODER_ACCOUNT_PRESET_ID;
  const presetEntries = useMemo(
    () => [
      { id: QODER_ACCOUNT_PRESET_ID, preset: qoderAccountPreset(app) },
      ...PRESETS,
    ],
    [app],
  );
  const stepState = useMemo<PresetStepState>(
    () => ({
      appId: app,
      step,
      setStep,
      host: pickerHost,
      // Qoder always supplies its selector; no selector-count fallback is needed.
      registerSelector: () => () => {},
    }),
    [app, step, pickerHost],
  );
  const [id, setId] = useState(originalId ?? "");
  const [config, setConfig] = useState<QoderProvider>(() =>
    formConfig(provider),
  );
  const [makeDefault, setMakeDefault] = useState(
    currentModel === "auto" ||
      Boolean(originalId && currentModel.startsWith(`${originalId}/`)),
  );
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const models = config.models ?? [];
  const update = (patch: Partial<QoderProvider>) =>
    setConfig((previous) => ({ ...previous, ...patch }));
  const updateModel = (index: number, patch: Partial<QoderModel>) =>
    update({
      models: models.map((model, position) =>
        position === index ? { ...model, ...patch } : model,
      ),
    });
  const duplicateId =
    !originalId && Object.prototype.hasOwnProperty.call(providers, id);
  const validId =
    /^[A-Za-z0-9][A-Za-z0-9._-]*$/.test(id) &&
    !["qoder", "constructor", "prototype"].includes(id);
  const validLimits = models.every((model) =>
    [model.contextWindow, model.maxOutputTokens].every(
      (value) =>
        value === undefined || (Number.isSafeInteger(value) && value > 0),
    ),
  );
  const ready =
    validId &&
    !duplicateId &&
    Boolean(config.baseUrl.trim()) &&
    models.length > 0 &&
    models.every((model) => model.model.trim()) &&
    new Set(models.map((model) => model.model.trim())).size === models.length &&
    validLimits;
  const submit = async (event: React.FormEvent) => {
    event.preventDefault();
    if (isAccountLogin || !ready || busy) return;
    setBusy(true);
    setError("");
    try {
      const cleaned = models.map((model) => ({
        ...model,
        model: model.model.trim(),
      }));
      const defaultModel = cleaned.some((model) => model.model === config.model)
        ? config.model
        : cleaned[0].model;
      await onSave(
        id,
        {
          ...config,
          baseUrl: config.baseUrl.trim(),
          model: defaultModel,
          models: cleaned,
        },
        makeDefault,
      );
      onClose();
    } catch (cause) {
      setError(extractErrorMessage(cause));
    } finally {
      setBusy(false);
    }
  };
  const canReturnToPicker = !originalId && !accountLogin && step === "form";
  const close = () => {
    if (busy) return;
    if (canReturnToPicker) setStep("pick");
    else onClose();
  };
  return (
    <FullScreenPanel
      isOpen
      trackUnsavedChanges={step === "form" && !isAccountLogin}
      title={t(
        originalId ? "qoder.editProviderTitle" : "provider.addNewProvider",
      )}
      subtitle={APP_DISPLAY_NAME[app]}
      backLabel={
        canReturnToPicker
          ? t("providerPreset.backToPick")
          : t("provider.backToList")
      }
      onClose={close}
      contentClassName={
        step === "pick" ? "flex h-full flex-col space-y-0 p-0" : "pt-4"
      }
      footer={
        step === "form" && !isAccountLogin ? (
          <>
            <Button
              type="button"
              variant="neutral"
              size="regular"
              disabled={busy}
              onClick={onClose}
            >
              {t("common.cancel")}
            </Button>
            <Button
              type="submit"
              form="qoder-provider-form"
              variant="solid"
              size="regular"
              disabled={!ready || busy}
            >
              {busy && <Loader2 className="h-3.5 w-3.5 animate-spin" />}
              {t("common.save")}
            </Button>
          </>
        ) : null
      }
    >
      <PresetStepContext.Provider value={stepState}>
        {step === "pick" && (
          <div ref={setPickerHost} className="min-h-0 flex-1" />
        )}
        <div hidden={step === "pick"}>
          <form
            id="qoder-provider-form"
            aria-label={t("qoder.editProviderTitle")}
            onSubmit={(event) => void submit(event)}
          >
            {!originalId && (
              <ProviderPresetSelector
                selectedPresetId={presetId}
                presetEntries={presetEntries}
                barAction={
                  isAccountLogin ? (
                    <div ref={setAccountActionHost} />
                  ) : undefined
                }
                onPresetChange={(selectedId) => {
                  const preset = presetEntries.find(
                    (entry) => entry.id === selectedId,
                  )?.preset;
                  setPresetId(selectedId);
                  if (selectedId === QODER_ACCOUNT_PRESET_ID) return;
                  setId(preset?.providerKey ?? "");
                  setConfig(
                    formConfig(
                      preset
                        ? {
                            ...structuredClone(preset.settingsConfig),
                            displayName: preset.nameKey
                              ? t(preset.nameKey)
                              : preset.name,
                          }
                        : undefined,
                    ),
                  );
                  setError("");
                }}
              />
            )}
            {isAccountLogin ? (
              step === "form" && (
                <QoderAccountLogin
                  app={app}
                  actionSlot={accountActionHost}
                  onUseAccount={async () => {
                    await onUseAccount();
                    onClose();
                  }}
                />
              )
            ) : (
              <fieldset
                disabled={busy}
                className={originalId ? "space-y-4" : "mt-6 space-y-4"}
              >
                <div className="grid grid-cols-2 gap-4">
                  <div className="space-y-1.5">
                    <Label htmlFor="qoder-provider-name">
                      {t("qoder.providerName")}
                    </Label>
                    <Input
                      id="qoder-provider-name"
                      value={config.displayName ?? ""}
                      onChange={(event) =>
                        update({ displayName: event.target.value })
                      }
                    />
                  </div>
                  <div className="space-y-1.5">
                    <Label htmlFor="qoder-provider-id">
                      {t("qoder.providerId")}
                    </Label>
                    <Input
                      id="qoder-provider-id"
                      value={id}
                      readOnly={Boolean(originalId)}
                      onChange={(event) => setId(event.target.value)}
                      placeholder="my-provider"
                      aria-invalid={duplicateId || (id !== "" && !validId)}
                    />
                    {(duplicateId || (id !== "" && !validId)) && (
                      <p className="text-caption text-danger-text">
                        {t(
                          duplicateId
                            ? "qoder.duplicateProvider"
                            : "qoder.invalidProviderId",
                        )}
                      </p>
                    )}
                  </div>
                </div>
                <div className="grid grid-cols-2 gap-4">
                  <div className="space-y-1.5">
                    <Label htmlFor="qoder-provider-protocol">
                      {t("qoder.protocol")}
                    </Label>
                    <Select
                      value={config.protocol ?? "openai"}
                      onValueChange={(
                        value: NonNullable<QoderProvider["protocol"]>,
                      ) =>
                        update({
                          protocol: value,
                          authType:
                            value === "anthropic" ? "api-key" : "bearer",
                        })
                      }
                    >
                      <SelectTrigger id="qoder-provider-protocol">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="openai">
                          OpenAI Chat Completions
                        </SelectItem>
                        <SelectItem value="openai-responses">
                          OpenAI Responses
                        </SelectItem>
                        <SelectItem value="anthropic">
                          Anthropic Messages
                        </SelectItem>
                      </SelectContent>
                    </Select>
                  </div>
                  <div className="space-y-1.5">
                    <Label htmlFor="qoder-provider-auth">
                      {t("qoder.authType")}
                    </Label>
                    <Select
                      value={config.authType ?? "bearer"}
                      onValueChange={(
                        value: NonNullable<QoderProvider["authType"]>,
                      ) => update({ authType: value })}
                    >
                      <SelectTrigger id="qoder-provider-auth">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="bearer">Bearer Token</SelectItem>
                        <SelectItem value="api-key">
                          API Key (x-api-key)
                        </SelectItem>
                      </SelectContent>
                    </Select>
                  </div>
                </div>
                <div className="space-y-1.5">
                  <Label htmlFor="qoder-provider-url">
                    {t("qoder.baseUrl")}
                  </Label>
                  <Input
                    id="qoder-provider-url"
                    type="url"
                    required
                    value={config.baseUrl}
                    onChange={(event) =>
                      update({ baseUrl: event.target.value })
                    }
                    placeholder="https://api.example.com/v1"
                  />
                </div>
                <div className="space-y-1.5">
                  <Label htmlFor="qoder-provider-key">API Key</Label>
                  <Input
                    id="qoder-provider-key"
                    type="password"
                    autoComplete="new-password"
                    value={config.apiKey}
                    onChange={(event) => update({ apiKey: event.target.value })}
                  />
                  <p className="text-caption text-fg-2">
                    {t("qoder.apiKeyHint")}
                  </p>
                </div>
                <div className="flex items-center justify-between">
                  <Label>{t("qoder.models")}</Label>
                  <Button
                    type="button"
                    variant="neutral"
                    size="compact"
                    onClick={() => update({ models: [...models, newModel()] })}
                  >
                    <Plus className="h-3.5 w-3.5" />
                    {t("qoder.addModel")}
                  </Button>
                </div>
                {models.map((model, index) => {
                  const thinking = model.capabilities?.thinking;
                  const mode =
                    thinking?.preferredMode ?? thinking?.modes?.[0] ?? "off";
                  return (
                    <div
                      key={index}
                      role="group"
                      aria-label={t("qoder.modelNumber", { number: index + 1 })}
                      className="space-y-3 rounded-control border border-border p-3"
                    >
                      <div className="flex items-end gap-3">
                        <div className="min-w-0 flex-1 space-y-1.5">
                          <Label htmlFor={`qoder-model-id-${index}`}>
                            {t("qoder.modelId")}
                          </Label>
                          <Input
                            id={`qoder-model-id-${index}`}
                            required
                            value={model.model}
                            onChange={(event) =>
                              updateModel(index, { model: event.target.value })
                            }
                            placeholder="model-id"
                          />
                        </div>
                        <div className="min-w-0 flex-1 space-y-1.5">
                          <Label htmlFor={`qoder-model-name-${index}`}>
                            {t("qoder.modelName")}
                          </Label>
                          <Input
                            id={`qoder-model-name-${index}`}
                            value={model.displayName ?? ""}
                            onChange={(event) =>
                              updateModel(index, {
                                displayName: event.target.value,
                              })
                            }
                          />
                        </div>
                        <Button
                          type="button"
                          variant="quiet"
                          size="icon-compact"
                          disabled={models.length <= 1}
                          aria-label={t("qoder.removeModel", {
                            number: index + 1,
                          })}
                          onClick={() =>
                            update({
                              models: models.filter(
                                (_, position) => position !== index,
                              ),
                            })
                          }
                        >
                          <Trash2 className="h-4 w-4" />
                        </Button>
                      </div>
                      <div className="grid grid-cols-2 gap-3">
                        {(["contextWindow", "maxOutputTokens"] as const).map(
                          (field) => (
                            <div key={field} className="space-y-1.5">
                              <Label htmlFor={`qoder-${field}-${index}`}>
                                {t(`qoder.${field}`)}
                              </Label>
                              <Input
                                id={`qoder-${field}-${index}`}
                                type="number"
                                min={1}
                                step={1}
                                value={model[field] ?? ""}
                                onChange={(event) =>
                                  updateModel(index, {
                                    [field]:
                                      event.target.value === ""
                                        ? undefined
                                        : Number(event.target.value),
                                  })
                                }
                              />
                            </div>
                          ),
                        )}
                      </div>
                      <div className="flex flex-wrap items-center gap-4">
                        {(["tools", "vision"] as const).map((field) => (
                          <label
                            key={field}
                            className="inline-flex items-center gap-2 text-body"
                          >
                            <Checkbox
                              checked={
                                model.capabilities?.[field] ?? field === "tools"
                              }
                              onCheckedChange={(checked) =>
                                updateModel(index, {
                                  capabilities: {
                                    ...model.capabilities,
                                    [field]: checked === true,
                                  },
                                })
                              }
                            />
                            {t(`qoder.${field}`)}
                          </label>
                        ))}
                        <div className="flex items-center gap-2">
                          <Label htmlFor={`qoder-thinking-${index}`}>
                            {t("qoder.thinking")}
                          </Label>
                          <Select
                            value={mode}
                            onValueChange={(value) =>
                              updateModel(index, {
                                capabilities: {
                                  ...model.capabilities,
                                  thinking: {
                                    ...thinking,
                                    modes: value === "off" ? [] : [value],
                                    preferredMode:
                                      value === "off" ? undefined : value,
                                    requiresBudgetForEnabled:
                                      value === "enabled" ? true : undefined,
                                  },
                                },
                              })
                            }
                          >
                            <SelectTrigger
                              id={`qoder-thinking-${index}`}
                              className="w-32"
                            >
                              <SelectValue />
                            </SelectTrigger>
                            <SelectContent>
                              <SelectItem value="off">
                                {t("qoder.thinkingOff")}
                              </SelectItem>
                              <SelectItem value="enabled">
                                {t("qoder.thinkingEnabled")}
                              </SelectItem>
                              <SelectItem value="adaptive">
                                {t("qoder.thinkingAdaptive")}
                              </SelectItem>
                            </SelectContent>
                          </Select>
                        </div>
                      </div>
                    </div>
                  );
                })}
                <div className="space-y-1.5">
                  <Label htmlFor="qoder-provider-default">
                    {t("qoder.providerDefaultModel")}
                  </Label>
                  <Select
                    value={
                      config.model &&
                      models.some((model) => model.model === config.model)
                        ? config.model
                        : models[0]?.model || ""
                    }
                    onValueChange={(model) => update({ model })}
                  >
                    <SelectTrigger id="qoder-provider-default">
                      <SelectValue
                        placeholder={t("qoder.selectModelPlaceholder")}
                      />
                    </SelectTrigger>
                    <SelectContent>
                      {models
                        .filter(
                          (model, index) =>
                            model.model.trim() &&
                            models.findIndex(
                              (entry) => entry.model === model.model,
                            ) === index,
                        )
                        .map((model, index) => (
                          <SelectItem key={index} value={model.model}>
                            {model.displayName || model.model}
                          </SelectItem>
                        ))}
                    </SelectContent>
                  </Select>
                </div>
                <label className="flex items-center gap-2 text-body">
                  <Checkbox
                    checked={makeDefault}
                    onCheckedChange={(checked) =>
                      setMakeDefault(checked === true)
                    }
                  />
                  {t("qoder.useAfterSave")}
                </label>
                {error && (
                  <p role="alert" className="text-body text-danger-text">
                    {error}
                  </p>
                )}
              </fieldset>
            )}
          </form>
        </div>
      </PresetStepContext.Provider>
    </FullScreenPanel>
  );
}
