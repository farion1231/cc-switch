import { useEffect, useState } from "react";
import { useForm } from "react-hook-form";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Form } from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import { ImeSafeInput } from "@/components/ui/ime-safe-input";
import { HelpTip } from "@/components/ui/help-tip";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { useProvidersQuery } from "@/lib/query/queries";
import type { ProviderFormData } from "@/lib/schemas/provider";
import { BasicFormFields } from "./BasicFormFields";
import { ApiKeySection, EndpointField, NativeConfigEditor } from "./shared";
import type { ProviderFormProps } from "./ProviderForm";

/**
 * One WorkBuddy provider is one entry of `models` in
 * `~/.workbuddy-ai/models.json`; `settings_config` holds that whole entry, so
 * every field CC Switch does not render here is still kept verbatim.
 */

const NUMBER_FIELDS = ["maxInputTokens", "maxOutputTokens"] as const;
type NumberField = (typeof NUMBER_FIELDS)[number];

const BOOLEAN_FIELDS = [
  "supportsToolCall",
  "supportsImages",
  "supportsReasoning",
] as const;
type BooleanField = (typeof BOOLEAN_FIELDS)[number];

type ModelConfig = Record<string, unknown>;

const readString = (config: ModelConfig, key: string): string => {
  const value = config[key];
  return typeof value === "string" ? value : "";
};

const readNumberText = (config: ModelConfig, key: NumberField): string => {
  const value = config[key];
  return typeof value === "number" && Number.isFinite(value)
    ? String(value)
    : "";
};

/**
 * `undefined` means "the file does not carry this flag". Keeping it undefined
 * until the user flips the switch avoids inventing a value WorkBuddy would
 * otherwise default itself.
 */
const readBoolean = (
  config: ModelConfig,
  key: BooleanField,
): boolean | undefined => {
  const value = config[key];
  return typeof value === "boolean" ? value : undefined;
};

const readRelatedModels = (config: ModelConfig): string => {
  const value = config.relatedModels;
  return Array.isArray(value)
    ? value
        .filter((item): item is string => typeof item === "string")
        .join(", ")
    : "";
};

/** A model served from this machine may legitimately need no key. */
const isLocalEndpoint = (url: string): boolean => {
  try {
    const host = new URL(url).hostname;
    return host === "localhost" || host === "127.0.0.1" || host === "::1";
  } catch {
    return false;
  }
};

export function WorkBuddyProviderForm({
  appId,
  providerId,
  initialData,
  onSubmit,
  onCancel,
  submitLabel,
  showButtons = true,
  onSubmittingChange,
  onSubmitReadyChange,
}: ProviderFormProps) {
  const { t } = useTranslation();
  const isEdit = Boolean(initialData);
  const [config, setConfig] = useState<ModelConfig>(
    () => (initialData?.settingsConfig ?? {}) as ModelConfig,
  );
  const [modelId, setModelId] = useState(
    () => providerId ?? readString(config, "id"),
  );
  const [vendor, setVendor] = useState(() => readString(config, "vendor"));
  const [tokens, setTokens] = useState<Record<NumberField, string>>(() => ({
    maxInputTokens: readNumberText(config, "maxInputTokens"),
    maxOutputTokens: readNumberText(config, "maxOutputTokens"),
  }));
  const [flags, setFlags] = useState<Record<BooleanField, boolean | undefined>>(
    () => ({
      supportsToolCall: readBoolean(config, "supportsToolCall"),
      supportsImages: readBoolean(config, "supportsImages"),
      supportsReasoning: readBoolean(config, "supportsReasoning"),
    }),
  );
  const [relatedModels, setRelatedModels] = useState(() =>
    readRelatedModels(config),
  );
  const [jsonText, setJsonText] = useState(() =>
    JSON.stringify(config, null, 2),
  );
  const [jsonValid, setJsonValid] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  const form = useForm<ProviderFormData>({
    defaultValues: {
      name: initialData?.name ?? "",
      notes: initialData?.notes ?? "",
      websiteUrl: initialData?.websiteUrl ?? "",
      icon: initialData?.icon ?? "",
      iconColor: initialData?.iconColor ?? "",
      settingsConfig: jsonText,
    },
  });
  const { data: existing } = useProvidersQuery(appId);
  const name = form.watch("name");

  const idTaken =
    !isEdit &&
    Object.prototype.hasOwnProperty.call(
      existing?.providers ?? {},
      modelId.trim(),
    );
  const idInvalid = !isEdit && modelId.trim() !== modelId;
  const apiKey = readString(config, "apiKey");
  const url = readString(config, "url");
  const needsApiKey = url.trim() !== "" && !isLocalEndpoint(url.trim());
  const ready = Boolean(
    jsonValid &&
      name.trim() &&
      modelId.trim() &&
      !idTaken &&
      !idInvalid &&
      url.trim() &&
      (apiKey.trim() || !needsApiKey),
  );

  useEffect(() => {
    onSubmitReadyChange?.(ready);
  }, [ready, onSubmitReadyChange]);

  const update = (next: ModelConfig) => {
    setConfig(next);
    setJsonText(JSON.stringify(next, null, 2));
    setJsonValid(true);
  };

  const patch = (changes: ModelConfig) => update({ ...config, ...changes });

  const dropEmpty = (key: string, value: string) => {
    const next = { ...config };
    if (value.trim()) next[key] = value;
    else delete next[key];
    update(next);
  };

  const setToken = (key: NumberField, text: string) => {
    setTokens((prev) => ({ ...prev, [key]: text }));
    const next = { ...config };
    const trimmed = text.trim();
    if (!trimmed) {
      delete next[key];
    } else {
      const parsed = Number(trimmed);
      if (!Number.isFinite(parsed) || parsed < 0) return;
      next[key] = Math.round(parsed);
    }
    update(next);
  };

  const setFlag = (key: BooleanField, value: boolean) => {
    setFlags((prev) => ({ ...prev, [key]: value }));
    patch({ [key]: value });
  };

  const commitRelatedModels = (text: string) => {
    setRelatedModels(text);
    const items = text
      .split(",")
      .map((item) => item.trim())
      .filter(Boolean);
    const next = { ...config };
    if (items.length) next.relatedModels = items;
    else delete next.relatedModels;
    update(next);
  };

  const flagRows: Array<{ key: BooleanField; label: string }> = [
    { key: "supportsToolCall", label: t("workbuddy.supportsToolCall") },
    { key: "supportsImages", label: t("workbuddy.supportsImages") },
    { key: "supportsReasoning", label: t("workbuddy.supportsReasoning") },
  ];

  return (
    <Form {...form}>
      <form
        id="provider-form"
        className="space-y-6"
        onSubmit={form.handleSubmit(async (identity) => {
          if (!ready || busy) return;
          setBusy(true);
          onSubmittingChange?.(true);
          setError("");
          try {
            const id = modelId.trim();
            const nextName = identity.name.trim();
            const nextUrl = url.trim();
            const next: ModelConfig = {
              ...config,
              id,
              name: nextName,
              url: nextUrl,
            };
            if (apiKey.trim()) next.apiKey = apiKey.trim();
            else delete next.apiKey;
            if (vendor.trim()) next.vendor = vendor.trim();
            else delete next.vendor;

            await onSubmit({
              ...identity,
              name: nextName,
              meta: initialData?.meta,
              providerKey: isEdit ? providerId : id,
              settingsConfig: JSON.stringify(next),
            });
          } catch (submitError) {
            setError(String(submitError));
          } finally {
            setBusy(false);
            onSubmittingChange?.(false);
          }
        })}
      >
        {error && (
          <p role="alert" className="text-sm text-destructive">
            {error}
          </p>
        )}
        <fieldset
          disabled={busy || !jsonValid}
          className="min-w-0 space-y-6 border-0 p-0 disabled:opacity-50"
        >
          <BasicFormFields
            form={form}
            beforeNameSlot={
              <div className="space-y-2">
                <div className="flex items-center gap-0.5">
                  <Label htmlFor="workbuddy-model-id">
                    {t("workbuddy.modelId")}
                    <span aria-hidden="true" className="text-destructive ml-1">
                      *
                    </span>
                  </Label>
                  <HelpTip title={t("workbuddy.modelId")}>
                    {isEdit
                      ? t("workbuddy.modelIdLocked")
                      : t("workbuddy.modelIdHint")}
                  </HelpTip>
                </div>
                <ImeSafeInput
                  id="workbuddy-model-id"
                  value={modelId}
                  onValueChange={setModelId}
                  disabled={isEdit}
                  autoComplete="off"
                  placeholder={t("workbuddy.modelIdPlaceholder")}
                  className={idTaken || idInvalid ? "border-destructive" : ""}
                />
                <p
                  className={
                    idTaken || idInvalid
                      ? "text-xs text-destructive"
                      : "text-xs text-fg-2"
                  }
                >
                  {idTaken
                    ? t("workbuddy.modelIdDuplicate")
                    : idInvalid
                      ? t("workbuddy.modelIdWhitespace")
                      : isEdit
                        ? t("workbuddy.modelIdLocked")
                        : t("workbuddy.modelIdHint")}
                </p>
              </div>
            }
          />

          <EndpointField
            id="workbuddy-url"
            label={t("providerForm.apiEndpoint")}
            value={url}
            onChange={(value) => dropEmpty("url", value)}
            placeholder={t("workbuddy.urlPlaceholder")}
            help={t("workbuddy.urlHelp")}
            showManageButton={false}
          />

          <ApiKeySection
            id="workbuddy-api-key"
            value={apiKey}
            onChange={(value) => dropEmpty("apiKey", value)}
            category={initialData?.category ?? "third_party"}
            shouldShowLink={false}
            websiteUrl={initialData?.websiteUrl ?? ""}
            required={needsApiKey}
            placeholder={{
              official: t("workbuddy.apiKeyLocalOptional"),
              thirdParty: t("workbuddy.apiKeyPlaceholder"),
            }}
          />

          <div className="space-y-2">
            <Label htmlFor="workbuddy-vendor">{t("workbuddy.vendor")}</Label>
            <Input
              id="workbuddy-vendor"
              value={vendor}
              onChange={(event) => {
                setVendor(event.target.value);
                dropEmpty("vendor", event.target.value);
              }}
              placeholder={t("workbuddy.vendorPlaceholder")}
              autoComplete="off"
            />
          </div>

          <div className="grid gap-4 sm:grid-cols-2">
            {NUMBER_FIELDS.map((key) => (
              <div key={key} className="space-y-2">
                <Label htmlFor={`workbuddy-${key}`}>
                  {t(`workbuddy.${key}`)}
                </Label>
                <Input
                  id={`workbuddy-${key}`}
                  inputMode="numeric"
                  value={tokens[key]}
                  onChange={(event) => setToken(key, event.target.value)}
                  placeholder={t("workbuddy.tokensPlaceholder")}
                  autoComplete="off"
                />
              </div>
            ))}
          </div>
          <p className="text-xs text-fg-2">{t("workbuddy.tokensHint")}</p>

          <div className="space-y-3">
            <Label>{t("workbuddy.capabilities")}</Label>
            {flagRows.map((row) => (
              <div
                key={row.key}
                className="flex items-center justify-between gap-4"
              >
                <span className="text-sm text-fg-1">{row.label}</span>
                <Switch
                  checked={flags[row.key] ?? false}
                  onCheckedChange={(value) => setFlag(row.key, value)}
                  aria-label={row.label}
                />
              </div>
            ))}
            <p className="text-xs text-fg-2">
              {t("workbuddy.capabilitiesHint")}
            </p>
          </div>

          <div className="space-y-2">
            <Label htmlFor="workbuddy-related-models">
              {t("workbuddy.relatedModels")}
            </Label>
            <Input
              id="workbuddy-related-models"
              value={relatedModels}
              onChange={(event) => commitRelatedModels(event.target.value)}
              placeholder={t("workbuddy.relatedModelsPlaceholder")}
              autoComplete="off"
            />
            <p className="text-xs text-fg-2">
              {t("workbuddy.relatedModelsHint")}
            </p>
          </div>
        </fieldset>

        {/* Reuses the additive-provider editor: it shows the real path the entry
            lives in, which is the only way to see where CC Switch writes. */}
        <NativeConfigEditor
          appId={appId}
          file="models.json"
          id="workbuddy-settings-config"
          value={jsonText}
          onChange={(text) => {
            setJsonText(text);
            try {
              const parsed = JSON.parse(text) as ModelConfig;
              if (
                !parsed ||
                typeof parsed !== "object" ||
                Array.isArray(parsed)
              ) {
                setJsonValid(false);
                return;
              }
              setConfig(parsed);
              setJsonValid(true);
            } catch {
              setJsonValid(false);
            }
          }}
          error={jsonValid ? null : t("workbuddy.rawConfigInvalid")}
        />

        {showButtons && (
          <div className="flex justify-end gap-2">
            <Button
              type="button"
              variant="outline"
              onClick={onCancel}
              disabled={busy}
            >
              {t("common.cancel")}
            </Button>
            <Button type="submit" disabled={busy || !ready}>
              {submitLabel}
            </Button>
          </div>
        )}
      </form>
    </Form>
  );
}
