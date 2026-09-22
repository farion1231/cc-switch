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
import {
  devecoProviderPresets,
  normalizeDevecoConfig as normalizeDevecoStoredConfig,
  DEVECO_NPM_PACKAGES,
} from "@/config/devEcoProviderPresets";
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

// DevEco 由 `npm` 决定 SDK，协议选择必须写 npm（Mcode 的 `api` 枚举在此无意义）。
const NPM_PACKAGES = DEVECO_NPM_PACKAGES.map(({ value, label }) => ({
  value,
  label,
}));
const DEFAULT_NPM = NPM_PACKAGES[0].value;

/**
 * 把历史上存成 Mcode `api` 枚举的配置迁移到 DevEco 的 `npm`。
 *
 * 旧数据没有 `npm`，有的是 `api: "anthropic-messages"` 之类。不迁移的话协议选择器
 * 读不到既有选择，编辑时也写不进 DevEco 真正用的字段。
 */
function normalizeDevecoConfig(
  stored: Record<string, unknown> | undefined,
): DevEcoConfig | undefined {
  return normalizeDevecoStoredConfig(stored) as DevEcoConfig | undefined;
}

const PRESET_ENTRIES = devecoProviderPresets.map((preset, index) => ({
  id: String(index),
  preset,
}));
const configSchema = z
  .object({
    // `npm` 决定 DevEco 加载哪个 AI SDK，是协议选择的落点。
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
type DevEcoConfig = z.infer<typeof configSchema>;

export function DevEcoProviderForm({
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
  const [config, setConfig] = useState<DevEcoConfig>(
    () =>
      // 编辑既有供应商时，旧配置存的是 Mcode 的 `api` 枚举；把它迁移到 npm，
      // 否则协议选择器显示的仍是旧值，改动也不会落到 DevEco 真正读取的字段上。
      normalizeDevecoConfig(initialData?.settingsConfig) ?? {
        npm: DEFAULT_NPM,
        options: {},
        models: {},
      },
  );
  const [jsonText, setJsonText] = useState(JSON.stringify(config, null, 2));
  const [jsonValid, setJsonValid] = useState(true);
  const [presetId, setPresetId] = useState("custom");
  const preset =
    presetId === "custom" ? undefined : devecoProviderPresets[Number(presetId)];
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
  const update = (next: DevEcoConfig) => {
    setConfig(next);
    setJsonText(JSON.stringify(next, null, 2));
    setJsonValid(true);
  };
  const updateOptions = (next: Partial<OpenCodeProviderOptions>) =>
    update({ ...config, options: { ...config.options, ...next } });
  const choosePreset = (id: string) => {
    setPresetId(id);
    const selected =
      id === "custom" ? undefined : devecoProviderPresets[Number(id)];
    const next = selected?.settingsConfig ?? {
      npm: DEFAULT_NPM,
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
            // 归一化已在初始状态做过：遗留的 `api` 枚举被迁到 npm，真实的
            // `api` URL（DevEco 原生字段）原样保留。这里只补上 name/kind。
            await onSubmit({
              ...identity,
              name: identity.name.trim(),
              meta: initialData?.meta,
              providerKey: providerId,
              presetCategory: category,
              settingsConfig: JSON.stringify({
                ...config,
                npm: config.npm ?? DEFAULT_NPM,
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
            apiFormats={NPM_PACKAGES}
            npm={config.npm ?? DEFAULT_NPM}
            onNpmChange={(npm) => update({ ...config, npm })}
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
          <Label htmlFor="deveco-settings-config">
            {t("provider.configJson")}
          </Label>
          <JsonEditor
            id="deveco-settings-config"
            ariaLabel={t("provider.configJson")}
            value={jsonText}
            darkMode={isDarkMode}
            language="json"
            showValidation
            height={Math.max(1, jsonText.split("\n").length) * 20 + 20}
            onChange={(text) => {
              setJsonText(text);
              try {
                // 与初始状态走同一条归一化：手改 JSON 里粘进来的 Mcode `api`
                // 枚举也要迁到 npm，否则下拉框显示 npm、保存却写回 api。
                const next = normalizeDevecoConfig(
                  configSchema.parse(JSON.parse(text)),
                )!;
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
