import { useEffect, useState } from "react";
import { z } from "zod";
import { useForm } from "react-hook-form";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Form } from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import JsonEditor from "@/components/JsonEditor";
import { useDarkMode } from "@/hooks/useDarkMode";
import { useProvidersQuery } from "@/lib/query/queries";
import type { ProviderFormData } from "@/lib/schemas/provider";
import { BasicFormFields } from "./BasicFormFields";
import { OpenCodeFormFields } from "./OpenCodeFormFields";
import { ProviderPresetSelector } from "./ProviderPresetSelector";
import { normalizeRequestHeaders } from "./helpers/requestHeaders";
import {
  mergeOpencodeExtraOptionRows,
  toOpencodeExtraOptions,
} from "./helpers/opencodeFormUtils";
import type { ProviderFormProps } from "./ProviderForm";

const API_FORMATS = [
  { value: "anthropic-messages", label: "Anthropic Messages" },
  { value: "openai-completions", label: "OpenAI Chat Completions" },
  { value: "openai-responses", label: "OpenAI Responses" },
];

const configSchema = z
  .object({
    npm: z.string().optional(),
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
type MimoConfig = z.infer<typeof configSchema>;
const KEY_PATTERN = /^[a-z0-9]+(-[a-z0-9]+)*$/;
const normalizeKey = (value: string) =>
  value.toLowerCase().replace(/[^a-z0-9-]/g, "");

export function MimoDesktopProviderForm({
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
  const [config, setConfig] = useState<MimoConfig>(
    initialData?.settingsConfig ?? {
      npm: "anthropic-messages",
      options: {},
      models: {},
    },
  );
  const [jsonText, setJsonText] = useState(JSON.stringify(config, null, 2));
  const [jsonValid, setJsonValid] = useState(true);
  const [extraOptions, setExtraOptions] = useState(
    toOpencodeExtraOptions(config.options ?? {}),
  );
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const isEdit = Boolean(initialData);
  const [providerKey, setProviderKey] = useState(providerId ?? "");
  const { data: existing } = useProvidersQuery("mimo-desktop");
  const keyTaken =
    !isEdit &&
    Object.prototype.hasOwnProperty.call(
      existing?.providers ?? {},
      providerKey,
    );
  const keyInvalid =
    !isEdit && providerKey !== "" && !KEY_PATTERN.test(providerKey);
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
      (isEdit || (providerKey && !keyInvalid && !keyTaken)) &&
      config.options?.baseURL?.trim() &&
      config.options?.apiKey?.trim() &&
      Object.keys(config.models ?? {}).length,
  );
  useEffect(() => {
    onSubmitReadyChange?.(ready);
  }, [ready, onSubmitReadyChange]);
  const update = (next: MimoConfig) => {
    setConfig(next);
    setJsonText(JSON.stringify(next, null, 2));
    setJsonValid(true);
  };
  const updateOptions = (next: Partial<typeof config.options>) =>
    update({ ...config, options: { ...config.options, ...next } });
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
              providerKey: isEdit ? providerId : providerKey,
              settingsConfig: JSON.stringify({
                ...config,
                name: identity.name.trim(),
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
                <Label htmlFor="mimo-provider-key">
                  {t("opencode.providerKey")}
                  <span aria-hidden="true" className="text-destructive ml-1">
                    *
                  </span>
                </Label>
                <Input
                  id="mimo-provider-key"
                  value={providerKey}
                  onChange={(event) =>
                    setProviderKey(normalizeKey(event.target.value))
                  }
                  disabled={isEdit}
                  placeholder={t("opencode.providerKeyPlaceholder")}
                  autoComplete="off"
                  className={keyTaken || keyInvalid ? "border-destructive" : ""}
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
                      ? t("opencode.providerKeyInvalid")
                      : isEdit
                        ? t("opencode.providerKeyLockedHint")
                        : t("opencode.providerKeyHint")}
                </p>
              </div>
            }
          />
          <OpenCodeFormFields
            apiFormats={API_FORMATS}
            npm={config.npm ?? "anthropic-messages"}
            onNpmChange={(npm) => update({ ...config, npm })}
            apiKey={config.options?.apiKey ?? ""}
            onApiKeyChange={(apiKey) => updateOptions({ apiKey })}
            category="custom"
            shouldShowApiKeyLink={false}
            websiteUrl=""
            isPartner={false}
            partnerPromotionKey={undefined}
            baseUrl={config.options?.baseURL ?? ""}
            onBaseUrlChange={(baseURL) => updateOptions({ baseURL })}
            headers={config.options?.headers ?? {}}
            onHeadersChange={(headers) => updateOptions({ headers })}
            models={(config.models ?? {}) as Record<string, any>}
            onModelsChange={(models) => update({ ...config, models })}
            extraOptions={extraOptions}
            onExtraOptionsChange={(draft) => {
              setExtraOptions(draft);
              const options: any = {
                ...(config.options ?? {}),
              };
              mergeOpencodeExtraOptionRows(options, draft);
              update({ ...config, options });
            }}
          />
        </fieldset>
        <div className="space-y-2">
          <Label htmlFor="mimo-settings-config">
            {t("provider.configJson")}
          </Label>
          <JsonEditor
            id="mimo-settings-config"
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