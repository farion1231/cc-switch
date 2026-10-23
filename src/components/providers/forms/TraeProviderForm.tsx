import { useCallback, useEffect, useState } from "react";
import { useForm } from "react-hook-form";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Form } from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import JsonEditor from "@/components/JsonEditor";
import { useDarkMode } from "@/hooks/useDarkMode";
import { useProvidersQuery } from "@/lib/query/queries";
import { toast } from "@/lib/toast";
import {
  fetchModelsForConfig,
  showFetchModelsError,
  type FetchedModel,
} from "@/lib/api/model-fetch";
import type { ProviderFormData } from "@/lib/schemas/provider";
import {
  isTraeOfficialConfig,
  normalizeTraeProviderKey,
  traeProviderPresets,
  TRAE_PROVIDER_KEY_PATTERN,
  TRAE_RESERVED_PROVIDER_KEYS,
  type TraeProviderConfig,
  type TraeProviderTable,
} from "@/config/traeProviderPresets";
import { BasicFormFields } from "./BasicFormFields";
import { ProviderPresetSelector } from "./ProviderPresetSelector";
import { ApiKeySection, EndpointField, ModelInputWithFetch } from "./shared";
import type { ProviderFormProps } from "./ProviderForm";

const PRESET_ENTRIES = traeProviderPresets.map((preset, index) => ({
  id: String(index),
  preset,
}));

type CustomConfig = { provider: TraeProviderTable; model: string };

const emptyConfig = (): CustomConfig => ({
  provider: {
    base_url: "",
    wire_api: "responses",
    experimental_bearer_token: "",
  },
  model: "",
});

const isCustomConfig = (value: unknown): value is CustomConfig =>
  typeof value === "object" &&
  value !== null &&
  typeof (value as { provider?: unknown }).provider === "object" &&
  (value as { provider?: unknown }).provider !== null;

export function TraeProviderForm({
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
  const isDarkMode = useDarkMode();
  const [config, setConfig] = useState<TraeProviderConfig>(
    (initialData?.settingsConfig as TraeProviderConfig | undefined) ??
      emptyConfig(),
  );
  const [jsonText, setJsonText] = useState(JSON.stringify(config, null, 2));
  const [jsonValid, setJsonValid] = useState(true);
  const [presetId, setPresetId] = useState("custom");
  const preset =
    presetId === "custom" ? undefined : traeProviderPresets[Number(presetId)];
  const official = isTraeOfficialConfig(config);
  const category = initialData?.category ?? preset?.category ?? "custom";
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [fetchedModels, setFetchedModels] = useState<FetchedModel[]>([]);
  const [isFetchingModels, setIsFetchingModels] = useState(false);
  // The key names `[model_providers.<key>]` and the top-level `model_provider`
  // reference, so it is fixed once the provider exists.
  const isEdit = Boolean(initialData);
  const [providerKey, setProviderKey] = useState(providerId ?? "");
  const { data: existing } = useProvidersQuery("trae");
  const keyTaken =
    !isEdit &&
    Object.prototype.hasOwnProperty.call(
      existing?.providers ?? {},
      providerKey,
    );
  const keyInvalid =
    !isEdit &&
    !official &&
    providerKey !== "" &&
    (!TRAE_PROVIDER_KEY_PATTERN.test(providerKey) ||
      TRAE_RESERVED_PROVIDER_KEYS.has(providerKey));
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
  const name = form.watch("name");
  const custom = isCustomConfig(config) ? config : undefined;
  const baseUrl = custom?.provider.base_url ?? "";
  const apiKey = custom?.provider.experimental_bearer_token ?? "";
  const envKey = custom?.provider.env_key ?? "";
  const ready = Boolean(
    jsonValid &&
      name.trim() &&
      (isEdit || (providerKey && !keyInvalid && !keyTaken)) &&
      (official ||
        (custom &&
          baseUrl.trim() &&
          custom.model.trim() &&
          (apiKey.trim() || envKey.trim()))),
  );
  useEffect(() => {
    onSubmitReadyChange?.(ready);
  }, [ready, onSubmitReadyChange]);

  const update = (next: TraeProviderConfig) => {
    setConfig(next);
    setJsonText(JSON.stringify(next, null, 2));
    setJsonValid(true);
  };
  const updateProvider = (patch: Partial<TraeProviderTable>) => {
    const current = custom ?? emptyConfig();
    const provider = { ...current.provider, ...patch };
    for (const [field, value] of Object.entries(patch)) {
      if (value === "" && field !== "base_url") delete provider[field];
    }
    update({ ...current, provider });
  };
  const choosePreset = (id: string) => {
    setPresetId(id);
    const selected =
      id === "custom" ? undefined : traeProviderPresets[Number(id)];
    const next = selected
      ? (JSON.parse(
          JSON.stringify(selected.settingsConfig),
        ) as TraeProviderConfig)
      : emptyConfig();
    update(next);
    setFetchedModels([]);
    setProviderKey(selected?.providerKey ?? "");
    form.reset({
      name: selected
        ? selected.nameKey
          ? t(selected.nameKey, { defaultValue: selected.name })
          : selected.name
        : "",
      notes: "",
      websiteUrl: selected?.websiteUrl ?? "",
      icon: selected?.icon ?? "",
      iconColor: selected?.iconColor ?? "",
      settingsConfig: JSON.stringify(next),
    });
  };
  const handleFetchModels = useCallback(() => {
    if (!baseUrl || !apiKey) {
      showFetchModelsError(null, t, {
        hasApiKey: !!apiKey,
        hasBaseUrl: !!baseUrl,
      });
      return;
    }
    setIsFetchingModels(true);
    fetchModelsForConfig(baseUrl, apiKey)
      .then((fetched) => {
        setFetchedModels(fetched);
        if (fetched.length === 0) {
          toast.info(t("providerForm.fetchModelsEmpty"));
        } else {
          toast.success(
            t("providerForm.fetchModelsSuccess", { count: fetched.length }),
          );
        }
      })
      .catch((err) => showFetchModelsError(err, t))
      .finally(() => setIsFetchingModels(false));
  }, [baseUrl, apiKey, t]);

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
            const settings: TraeProviderConfig = custom
              ? {
                  ...custom,
                  model: custom.model.trim(),
                  provider: {
                    ...custom.provider,
                    name: identity.name.trim(),
                    base_url: custom.provider.base_url.trim(),
                    wire_api: "responses",
                  },
                }
              : { official: true };
            await onSubmit({
              ...identity,
              name: identity.name.trim(),
              meta: initialData?.meta,
              providerKey: isEdit ? providerId : providerKey,
              presetCategory: category,
              settingsConfig: JSON.stringify(settings),
            });
          } catch (error) {
            setError(String(error));
          } finally {
            setBusy(false);
            onSubmittingChange?.(false);
          }
        })}
      >
        {!initialData && (
          <ProviderPresetSelector
            selectedPresetId={presetId}
            presetEntries={PRESET_ENTRIES}
            onPresetChange={choosePreset}
            category={category}
          />
        )}
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
              official ? undefined : (
                <div className="space-y-2">
                  <Label htmlFor="trae-provider-key">
                    {t("opencode.providerKey")}
                    <span aria-hidden="true" className="text-destructive ml-1">
                      *
                    </span>
                  </Label>
                  <Input
                    id="trae-provider-key"
                    value={providerKey}
                    onChange={(event) =>
                      setProviderKey(
                        normalizeTraeProviderKey(event.target.value),
                      )
                    }
                    disabled={isEdit}
                    placeholder={t("opencode.providerKeyPlaceholder")}
                    autoComplete="off"
                    className={
                      keyTaken || keyInvalid ? "border-destructive" : ""
                    }
                  />
                  <p
                    className={
                      keyTaken || keyInvalid
                        ? "text-xs text-destructive"
                        : "text-xs text-fg-2"
                    }
                  >
                    {keyTaken
                      ? t("opencode.providerKeyDuplicate")
                      : keyInvalid
                        ? t("trae.providerKeyInvalid")
                        : isEdit
                          ? t("opencode.providerKeyLockedHint")
                          : t("trae.providerKeyHint")}
                  </p>
                </div>
              )
            }
          />
          {official ? (
            <div className="space-y-2">
              <p className="rounded-md border border-border-default bg-subtle p-3 text-sm text-fg-2">
                {t("trae.officialHint")}
              </p>
              {keyTaken && (
                <p className="text-xs text-destructive">
                  {t("trae.officialExists")}
                </p>
              )}
            </div>
          ) : (
            <>
              <EndpointField
                id="trae-base-url"
                label={t("providerForm.apiEndpoint")}
                value={baseUrl}
                onChange={(value) => updateProvider({ base_url: value })}
                placeholder="https://api.example.com/v1"
                hint={t("trae.responsesOnlyHint")}
                required
              />
              <ApiKeySection
                id="trae-api-key"
                value={apiKey}
                onChange={(value) =>
                  updateProvider({ experimental_bearer_token: value })
                }
                category={category}
                shouldShowLink={Boolean(preset?.apiKeyUrl)}
                websiteUrl={preset?.apiKeyUrl ?? ""}
                isPartner={preset?.isPartner}
                partnerPromotionKey={preset?.partnerPromotionKey}
                required={!envKey.trim()}
              />
              <div className="space-y-2">
                <Label htmlFor="trae-env-key">{t("trae.envKey")}</Label>
                <Input
                  id="trae-env-key"
                  value={envKey}
                  onChange={(event) =>
                    updateProvider({ env_key: event.target.value.trim() })
                  }
                  placeholder="MY_PROVIDER_API_KEY"
                  autoComplete="off"
                />
                <p className="text-xs text-fg-2">{t("trae.envKeyHint")}</p>
              </div>
              <div className="space-y-2">
                <Label htmlFor="trae-model">
                  {t("trae.model")}
                  <span aria-hidden="true" className="text-destructive ml-1">
                    *
                  </span>
                </Label>
                <ModelInputWithFetch
                  id="trae-model"
                  value={custom?.model ?? ""}
                  onChange={(model) =>
                    update({ ...(custom ?? emptyConfig()), model })
                  }
                  placeholder="gpt-5.1-codex"
                  fetchedModels={fetchedModels}
                  isLoading={isFetchingModels}
                  onFetch={handleFetchModels}
                />
              </div>
            </>
          )}
        </fieldset>
        {!official && (
          <div className="space-y-2">
            <Label htmlFor="trae-settings-config">
              {t("provider.configJson")}
            </Label>
            <JsonEditor
              id="trae-settings-config"
              ariaLabel={t("provider.configJson")}
              value={jsonText}
              darkMode={isDarkMode}
              language="json"
              showValidation
              height={Math.max(1, jsonText.split("\n").length) * 20 + 20}
              onChange={(text) => {
                setJsonText(text);
                try {
                  const next = JSON.parse(text);
                  if (
                    !isCustomConfig(next) ||
                    typeof next.model !== "string" ||
                    typeof next.provider.base_url !== "string"
                  ) {
                    throw new Error("invalid");
                  }
                  setConfig(next);
                  setJsonValid(true);
                } catch {
                  setJsonValid(false);
                }
              }}
            />
          </div>
        )}
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
