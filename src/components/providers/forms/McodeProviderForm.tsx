import { useEffect, useState } from "react";
import { z } from "zod";
import { useForm } from "react-hook-form";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Form } from "@/components/ui/form";
import { Label } from "@/components/ui/label";
import JsonEditor from "@/components/JsonEditor";
import { useDarkMode } from "@/hooks/useDarkMode";
import type { ProviderFormData } from "@/lib/schemas/provider";
import type { OpenCodeModel, OpenCodeProviderOptions } from "@/types";
import { mcodeProviderPresets } from "@/config/mcodeProviderPresets";
import { BasicFormFields } from "./BasicFormFields";
import { OpenCodeFormFields } from "./OpenCodeFormFields";
import { ProviderPresetSelector } from "./ProviderPresetSelector";
import { normalizeRequestHeaders } from "./helpers/requestHeaders";
import {
  isKnownOpencodeOptionKey,
  OPENCODE_EXTRA_OPTION_DRAFT_PREFIX,
  toOpencodeExtraOptions,
} from "./helpers/opencodeFormUtils";
import type { ProviderFormProps } from "./ProviderForm";

const API_FORMATS = [
  { value: "anthropic-messages", label: "Anthropic Messages" },
  { value: "openai-completions", label: "OpenAI Chat Completions" },
  { value: "openai-responses", label: "OpenAI Responses" },
];
const PRESET_ENTRIES = mcodeProviderPresets.map((preset, index) => ({
  id: String(index),
  preset,
}));
const configSchema = z
  .object({
    api: z.string().optional(),
    options: z
      .object({
        baseURL: z.string().optional(),
        apiKey: z.string().optional(),
        headers: z.record(z.string(), z.string()).optional(),
      })
      .passthrough()
      .optional(),
    models: z
      .record(
        z.string(),
        z
          .object({
            name: z.string().optional(),
            limit: z
              .object({
                context: z.number().optional(),
                output: z.number().optional(),
              })
              .passthrough()
              .optional(),
            options: z.record(z.string(), z.unknown()).optional(),
          })
          .passthrough(),
      )
      .optional(),
  })
  .passthrough();
type McodeConfig = z.infer<typeof configSchema>;

export function McodeProviderForm({
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
  const [config, setConfig] = useState<McodeConfig>(
    initialData?.settingsConfig ?? {
      kind: "custom",
      enabled: true,
      api: "anthropic-messages",
      options: {},
      models: {},
    },
  );
  const [jsonText, setJsonText] = useState(JSON.stringify(config, null, 2));
  const [jsonValid, setJsonValid] = useState(true);
  const [presetId, setPresetId] = useState("custom");
  const preset =
    presetId === "custom" ? undefined : mcodeProviderPresets[Number(presetId)];
  const category = initialData?.category ?? preset?.category ?? "custom";
  const [extraOptions, setExtraOptions] = useState(
    toOpencodeExtraOptions(config.options ?? {}),
  );
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
  const name = form.watch("name");
  const ready = Boolean(
    jsonValid &&
      name.trim() &&
      config.options?.baseURL?.trim() &&
      config.options?.apiKey?.trim() &&
      Object.keys(config.models ?? {}).length,
  );
  useEffect(() => {
    onSubmitReadyChange?.(ready);
  }, [ready, onSubmitReadyChange]);
  const update = (next: McodeConfig) => {
    setConfig(next);
    setJsonText(JSON.stringify(next, null, 2));
    setJsonValid(true);
  };
  const updateOptions = (next: Partial<OpenCodeProviderOptions>) =>
    update({ ...config, options: { ...config.options, ...next } });
  const choosePreset = (id: string) => {
    setPresetId(id);
    const selected =
      id === "custom" ? undefined : mcodeProviderPresets[Number(id)];
    const next = selected?.settingsConfig ?? {
      kind: "custom",
      enabled: true,
      api: "anthropic-messages",
      options: {},
      models: {},
    };
    update(next);
    setExtraOptions(toOpencodeExtraOptions(next.options));
    form.reset({
      name: selected?.name ?? "",
      notes: "",
      websiteUrl: selected?.websiteUrl ?? "",
      icon: selected?.icon ?? "",
      iconColor: selected?.iconColor ?? "",
      settingsConfig: JSON.stringify(next),
    });
  };
  return (
    <Form {...form}>
      <form
        id="provider-form"
        className="space-y-6 glass rounded-xl p-6 border border-white/10"
        onSubmit={form.handleSubmit(async (identity) => {
          if (!ready || busy) return;
          setBusy(true);
          onSubmittingChange?.(true);
          setError("");
          try {
            const headers = normalizeRequestHeaders(
              config.options?.headers ?? {},
            );
            const options = { ...config.options };
            if (Object.keys(headers).length) options.headers = headers;
            else delete options.headers;
            await onSubmit({
              ...identity,
              name: identity.name.trim(),
              meta: initialData?.meta,
              providerKey: providerId,
              presetCategory: category,
              settingsConfig: JSON.stringify({
                ...config,
                name: identity.name.trim(),
                kind: "custom",
                options,
              }),
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
            presetCategoryLabels={{
              custom: t("providerPreset.custom"),
              cn_official: t("providerForm.categoryCnOfficial"),
              aggregator: t("providerForm.categoryAggregation"),
              third_party: t("providerForm.categoryThirdParty"),
            }}
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
          <BasicFormFields form={form} />
          <OpenCodeFormFields
            apiFormats={API_FORMATS}
            npm={config.api ?? "anthropic-messages"}
            onNpmChange={(api) => update({ ...config, api })}
            apiKey={config.options?.apiKey ?? ""}
            onApiKeyChange={(apiKey) => updateOptions({ apiKey })}
            category={category}
            shouldShowApiKeyLink={Boolean(preset?.apiKeyUrl)}
            websiteUrl={preset?.apiKeyUrl ?? ""}
            isPartner={preset?.isPartner}
            partnerPromotionKey={preset?.partnerPromotionKey}
            baseUrl={config.options?.baseURL ?? ""}
            onBaseUrlChange={(baseURL) => updateOptions({ baseURL })}
            headers={config.options?.headers ?? {}}
            onHeadersChange={(headers) => updateOptions({ headers })}
            models={(config.models ?? {}) as Record<string, OpenCodeModel>}
            onModelsChange={(models) => update({ ...config, models })}
            extraOptions={extraOptions}
            onExtraOptionsChange={(draft) => {
              setExtraOptions(draft);
              const options = Object.fromEntries(
                Object.entries(config.options ?? {}).filter(([key]) =>
                  isKnownOpencodeOptionKey(key),
                ),
              );
              for (const [key, value] of Object.entries(draft)) {
                if (
                  !key.trim() ||
                  key.startsWith(OPENCODE_EXTRA_OPTION_DRAFT_PREFIX)
                )
                  continue;
                try {
                  options[key.trim()] = JSON.parse(value);
                } catch {
                  options[key.trim()] = value;
                }
              }
              update({ ...config, options });
            }}
          />
        </fieldset>
        <div className="space-y-2">
          <Label htmlFor="mcode-settings-config">
            {t("provider.configJson")}
          </Label>
          <JsonEditor
            id="mcode-settings-config"
            ariaLabel={t("provider.configJson")}
            value={jsonText}
            darkMode={isDarkMode}
            language="json"
            showValidation
            height={Math.max(1, jsonText.split("\n").length) * 20 + 20}
            onChange={(text) => {
              setJsonText(text);
              try {
                const next = configSchema.parse(JSON.parse(text));
                setConfig(next);
                setExtraOptions(toOpencodeExtraOptions(next.options ?? {}));
                setJsonValid(true);
              } catch {
                setJsonValid(false);
              }
            }}
          />
        </div>
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
