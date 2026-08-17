import { useEffect, useMemo, useState } from "react";
import { useForm } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import {
  Form,
  FormControl,
  FormField,
  FormItem,
  FormLabel,
  FormMessage,
} from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Textarea } from "@/components/ui/textarea";
import JsonEditor from "@/components/JsonEditor";
import { useDarkMode } from "@/hooks/useDarkMode";
import { providerSchema, type ProviderFormData } from "@/lib/schemas/provider";
import ApiKeyInput from "./ApiKeyInput";
import { BasicFormFields } from "./BasicFormFields";
import type { ProviderFormProps, ProviderFormValues } from "./ProviderForm";

type DeepSeekHarnessProviderFormProps = Omit<ProviderFormProps, "appId">;

export type DeepSeekThinking = "enabled" | "disabled";
export type DeepSeekReasoningEffort = "off" | "low" | "high" | "max";

export interface DeepSeekHarnessModel {
  id: string;
  name?: string;
  description?: string;
  contextWindow?: number;
  maxTokens?: number;
  [key: string]: unknown;
}

export interface DeepSeekHarnessSettingsConfig {
  apiKey?: string;
  defaultModel?: string;
  defaultReasoningEffort?: DeepSeekReasoningEffort;
  apiKeyEnv?: string;
  baseURL?: string;
  thinking?: DeepSeekThinking;
  reasoningEffort?: DeepSeekReasoningEffort;
  maxTokens?: number;
  defaultContextWindow?: number;
  models?: DeepSeekHarnessModel[];
  [key: string]: unknown;
}

const PRIVATE_PROFILE_FIELDS = new Set([
  "apiKey",
  "defaultModel",
  "defaultReasoningEffort",
]);

export const nativeDeepSeekHarnessProfile = (
  config: DeepSeekHarnessSettingsConfig,
): Record<string, unknown> =>
  Object.fromEntries(
    Object.entries(config).filter(([key]) => !PRIVATE_PROFILE_FIELDS.has(key)),
  );

const DEFAULT_BASE_URL = "https://api.deepseek.com";
const DEFAULT_API_KEY_ENV = "DEEPSEEK_API_KEY";
const DEFAULT_MODEL = "deepseek-v4-flash";

const isRecord = (value: unknown): value is Record<string, unknown> =>
  value !== null && typeof value === "object" && !Array.isArray(value);

const parseModelsText = (value: string): DeepSeekHarnessModel[] => {
  const seen = new Set<string>();
  const models: DeepSeekHarnessModel[] = [];
  for (const line of value.split(/\r?\n/)) {
    const id = line.trim();
    if (!id || seen.has(id)) continue;
    seen.add(id);
    models.push({ id });
  }
  return models;
};

const stringifyModels = (value: unknown): string =>
  Array.isArray(value)
    ? value
        .map((model) =>
          isRecord(model) && typeof model.id === "string" ? model.id : "",
        )
        .filter(Boolean)
        .join("\n")
    : "";

/**
 * Applies form-owned fields on top of the raw object so editing an advanced
 * Harness profile never discards fields CC Switch does not render yet.
 */
export function mergeDeepSeekHarnessConfig(
  raw: Record<string, unknown>,
  fields: {
    apiKey: string;
    includeApiKey: boolean;
    defaultModel: string;
    apiKeyEnv: string;
    baseURL: string;
    thinking: DeepSeekThinking;
    defaultReasoningEffort: DeepSeekReasoningEffort;
    includeDefaultReasoningEffort: boolean;
    modelsText: string;
  },
): DeepSeekHarnessSettingsConfig {
  const currentModels = Array.isArray(raw.models) ? raw.models : [];
  const existingById = new Map<string, DeepSeekHarnessModel>();
  for (const model of currentModels) {
    if (isRecord(model) && typeof model.id === "string") {
      existingById.set(model.id, model as DeepSeekHarnessModel);
    }
  }

  const models = parseModelsText(fields.modelsText).map((model) => ({
    ...(existingById.get(model.id) ?? {}),
    id: model.id,
  }));
  const merged: DeepSeekHarnessSettingsConfig = {
    ...raw,
    defaultModel: fields.defaultModel.trim(),
    apiKeyEnv: fields.apiKeyEnv.trim(),
    baseURL: fields.baseURL.trim(),
    thinking: fields.thinking,
    models,
  };

  // Absence means "keep the referenced native credential" while an explicit
  // empty string means "unset it". Do not collapse those two operations.
  if (fields.includeApiKey) {
    merged.apiKey = fields.apiKey.trim();
  }

  // The structured effort control owns agent-default-model.reasoningEffort,
  // not llm-deepseek.reasoningEffort. Preserve the profile-level value from
  // the advanced object unless disabling thinking requires correcting it.
  if (fields.includeDefaultReasoningEffort) {
    merged.defaultReasoningEffort =
      fields.thinking === "disabled" ? "off" : fields.defaultReasoningEffort;
  }
  if (
    fields.thinking === "disabled" &&
    Object.prototype.hasOwnProperty.call(raw, "reasoningEffort")
  ) {
    merged.reasoningEffort = "off";
  }

  return merged;
}

export function DeepSeekHarnessProviderForm({
  submitLabel,
  onSubmit,
  onCancel,
  onSubmittingChange,
  initialData,
  showButtons = true,
}: DeepSeekHarnessProviderFormProps) {
  const { t } = useTranslation();
  const isDarkMode = useDarkMode();
  const initialConfig = useMemo<DeepSeekHarnessSettingsConfig>(() => {
    return isRecord(initialData?.settingsConfig)
      ? (initialData.settingsConfig as DeepSeekHarnessSettingsConfig)
      : {};
  }, [initialData?.settingsConfig]);

  const [apiKey, setApiKey] = useState(
    typeof initialConfig.apiKey === "string" ? initialConfig.apiKey : "",
  );
  const [apiKeyTouched, setApiKeyTouched] = useState(false);
  const initialHasApiKey = Object.prototype.hasOwnProperty.call(
    initialConfig,
    "apiKey",
  );
  const [apiKeyEnv, setApiKeyEnv] = useState(
    typeof initialConfig.apiKeyEnv === "string"
      ? initialConfig.apiKeyEnv
      : DEFAULT_API_KEY_ENV,
  );
  const [baseURL, setBaseURL] = useState(
    typeof initialConfig.baseURL === "string" ? initialConfig.baseURL : "",
  );
  const [defaultModel, setDefaultModel] = useState(
    typeof initialConfig.defaultModel === "string"
      ? initialConfig.defaultModel
      : DEFAULT_MODEL,
  );
  const [thinking, setThinking] = useState<DeepSeekThinking>(
    initialConfig.thinking === "disabled" ? "disabled" : "enabled",
  );
  const [thinkingConfigured, setThinkingConfigured] = useState(
    initialConfig.thinking === "enabled" ||
      initialConfig.thinking === "disabled",
  );
  const [defaultReasoningEffort, setDefaultReasoningEffort] =
    useState<DeepSeekReasoningEffort>(
      initialConfig.defaultReasoningEffort === "off" ||
        initialConfig.defaultReasoningEffort === "low" ||
        initialConfig.defaultReasoningEffort === "high" ||
        initialConfig.defaultReasoningEffort === "max"
        ? initialConfig.defaultReasoningEffort
        : initialConfig.reasoningEffort === "off" ||
            initialConfig.reasoningEffort === "low" ||
            initialConfig.reasoningEffort === "max"
          ? initialConfig.reasoningEffort
          : "high",
    );
  const [defaultReasoningConfigured, setDefaultReasoningConfigured] = useState(
    initialConfig.defaultReasoningEffort === "off" ||
      initialConfig.defaultReasoningEffort === "low" ||
      initialConfig.defaultReasoningEffort === "high" ||
      initialConfig.defaultReasoningEffort === "max",
  );
  const [thinkingTouched, setThinkingTouched] = useState(false);
  const [modelsText, setModelsText] = useState(
    stringifyModels(initialConfig.models),
  );
  const [rawConfig, setRawConfig] = useState(() =>
    JSON.stringify(nativeDeepSeekHarnessProfile(initialConfig), null, 2),
  );
  const [rawError, setRawError] = useState<string>();

  const form = useForm<ProviderFormData>({
    resolver: zodResolver(providerSchema),
    defaultValues: {
      name:
        initialData?.name ??
        t("deepseekHarness.defaultProviderName", {
          defaultValue: "DeepSeek Official",
        }),
      websiteUrl: initialData?.websiteUrl ?? "https://platform.deepseek.com/",
      notes: initialData?.notes ?? "",
      settingsConfig: JSON.stringify(initialConfig),
      icon: initialData?.icon ?? "deepseek",
      iconColor: initialData?.iconColor ?? "#4D6BFE",
    },
    mode: "onSubmit",
  });
  const { isSubmitting } = form.formState;

  useEffect(() => {
    onSubmittingChange?.(isSubmitting);
  }, [isSubmitting, onSubmittingChange]);

  const applyRawConfig = (value: string) => {
    setRawConfig(value);
    try {
      const parsed = JSON.parse(value) as unknown;
      if (!isRecord(parsed)) {
        throw new Error(
          t("jsonEditor.mustBeObject", {
            defaultValue: "Configuration must be a JSON object",
          }),
        );
      }
      setRawError(undefined);
      // The advanced editor owns only fields not represented above. Keeping
      // structured controls as the single source of truth prevents editing an
      // unrelated advanced field from silently restoring stale endpoint,
      // model, or credential values from the initial JSON snapshot.
    } catch (error) {
      setRawError(error instanceof Error ? error.message : String(error));
    }
  };

  const handleSubmit = async (values: ProviderFormData) => {
    if (!values.name.trim()) {
      toast.error(t("provider.nameRequired"));
      return;
    }
    if (!defaultModel.trim()) {
      toast.error(
        t("deepseekHarness.defaultModelRequired", {
          defaultValue: "Default model is required",
        }),
      );
      return;
    }
    if (!apiKeyEnv.trim()) {
      toast.error(
        t("deepseekHarness.apiKeyEnvRequired", {
          defaultValue: "Credential reference is required",
        }),
      );
      return;
    }

    let advanced: Record<string, unknown>;
    try {
      const parsed = JSON.parse(rawConfig) as unknown;
      if (!isRecord(parsed)) throw new Error("JSON must be an object");
      advanced = parsed;
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setRawError(message);
      toast.error(
        t("deepseekHarness.invalidAdvancedJson", {
          error: message,
          defaultValue: `Invalid advanced JSON: ${message}`,
        }),
      );
      return;
    }

    const config = mergeDeepSeekHarnessConfig(advanced, {
      apiKey,
      includeApiKey: initialHasApiKey || apiKeyTouched,
      defaultModel,
      apiKeyEnv,
      baseURL,
      thinking,
      defaultReasoningEffort,
      includeDefaultReasoningEffort:
        defaultReasoningConfigured ||
        (thinkingTouched && thinking === "disabled"),
      modelsText,
    });
    if (!baseURL.trim()) delete config.baseURL;
    if (!thinkingConfigured) delete config.thinking;
    if (
      !defaultReasoningConfigured &&
      !(thinkingTouched && thinking === "disabled")
    ) {
      delete config.defaultReasoningEffort;
    }
    if (!Array.isArray(advanced.models) && !modelsText.trim()) {
      delete config.models;
    }
    await onSubmit({
      ...values,
      name: values.name.trim(),
      websiteUrl: values.websiteUrl?.trim() ?? "",
      notes: values.notes?.trim() ?? "",
      settingsConfig: JSON.stringify(config),
      presetCategory: "official",
    } satisfies ProviderFormValues);
  };

  return (
    <Form {...form}>
      <form
        id="provider-form"
        onSubmit={form.handleSubmit(handleSubmit)}
        className="space-y-6 glass rounded-xl p-6 border border-white/10"
      >
        <BasicFormFields form={form} />

        <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
          <ApiKeyInput
            id="deepseek-harness-api-key"
            label={t("deepseekHarness.apiKey", { defaultValue: "API Key" })}
            value={apiKey}
            onChange={(value) => {
              setApiKey(value);
              setApiKeyTouched(true);
            }}
            placeholder="sk-..."
          />
          <FormItem>
            <FormLabel htmlFor="deepseek-harness-api-key-env">
              {t("deepseekHarness.apiKeyEnv", {
                defaultValue: "Credential reference",
              })}
            </FormLabel>
            <Input
              id="deepseek-harness-api-key-env"
              value={apiKeyEnv}
              onChange={(event) => setApiKeyEnv(event.target.value)}
              placeholder={DEFAULT_API_KEY_ENV}
              autoComplete="off"
            />
          </FormItem>

          <FormItem>
            <FormLabel htmlFor="deepseek-harness-base-url">
              {t("deepseekHarness.baseURL", { defaultValue: "Base URL" })}
            </FormLabel>
            <Input
              id="deepseek-harness-base-url"
              value={baseURL}
              onChange={(event) => setBaseURL(event.target.value)}
              placeholder={DEFAULT_BASE_URL}
              autoComplete="off"
            />
          </FormItem>
          <FormItem>
            <FormLabel htmlFor="deepseek-harness-default-model">
              {t("deepseekHarness.defaultModel", {
                defaultValue: "Default model",
              })}
            </FormLabel>
            <Input
              id="deepseek-harness-default-model"
              value={defaultModel}
              onChange={(event) => setDefaultModel(event.target.value)}
              placeholder={DEFAULT_MODEL}
              autoComplete="off"
            />
          </FormItem>

          <FormItem>
            <FormLabel htmlFor="deepseek-harness-thinking">
              {t("deepseekHarness.thinking", {
                defaultValue: "Thinking policy",
              })}
            </FormLabel>
            <Select
              value={thinking}
              onValueChange={(value) => {
                const next = value as DeepSeekThinking;
                setThinking(next);
                setThinkingConfigured(true);
                setThinkingTouched(true);
              }}
            >
              <SelectTrigger id="deepseek-harness-thinking">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="enabled">
                  {t("common.enabled", { defaultValue: "Enabled" })}
                </SelectItem>
                <SelectItem value="disabled">
                  {t("deepseekHarness.disabled", { defaultValue: "Disabled" })}
                </SelectItem>
              </SelectContent>
            </Select>
          </FormItem>
          <FormItem>
            <FormLabel htmlFor="deepseek-harness-reasoning-effort">
              {t("deepseekHarness.reasoningEffort", {
                defaultValue: "Default reasoning effort",
              })}
            </FormLabel>
            <Select
              value={thinking === "disabled" ? "off" : defaultReasoningEffort}
              disabled={thinking === "disabled"}
              onValueChange={(value) => {
                setDefaultReasoningEffort(value as DeepSeekReasoningEffort);
                setDefaultReasoningConfigured(true);
              }}
            >
              <SelectTrigger id="deepseek-harness-reasoning-effort">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="off">off</SelectItem>
                <SelectItem value="low">low</SelectItem>
                <SelectItem value="high">high</SelectItem>
                <SelectItem value="max">max</SelectItem>
              </SelectContent>
            </Select>
          </FormItem>
        </div>

        <FormItem>
          <FormLabel htmlFor="deepseek-harness-models">
            {t("deepseekHarness.models", {
              defaultValue: "Advisory models (one ID per line)",
            })}
          </FormLabel>
          <Textarea
            id="deepseek-harness-models"
            value={modelsText}
            onChange={(event) => setModelsText(event.target.value)}
            placeholder={"deepseek-v4-flash\ndeepseek-v4-pro"}
            className="font-mono"
          />
          <p className="text-xs text-muted-foreground">
            {t("deepseekHarness.modelsHint", {
              defaultValue:
                "The selected default model may also be an unlisted pass-through ID.",
            })}
          </p>
        </FormItem>

        <FormItem>
          <FormLabel htmlFor="deepseek-harness-advanced-json">
            {t("deepseekHarness.advancedJson", {
              defaultValue: "Advanced llm-deepseek profile (JSON)",
            })}
          </FormLabel>
          <JsonEditor
            id="deepseek-harness-advanced-json"
            value={rawConfig}
            onChange={applyRawConfig}
            darkMode={isDarkMode}
            rows={8}
          />
          <p className="text-xs text-muted-foreground">
            {t("deepseekHarness.advancedHint", {
              defaultValue:
                "Unknown native profile fields are retained. Structured fields above take precedence when saving.",
            })}
          </p>
          {rawError ? (
            <p className="text-xs text-destructive">{rawError}</p>
          ) : null}
        </FormItem>

        <FormField
          control={form.control}
          name="settingsConfig"
          render={() => (
            <FormItem className="hidden">
              <FormControl>
                <Input type="hidden" />
              </FormControl>
              <FormMessage />
            </FormItem>
          )}
        />

        {showButtons && (
          <div className="flex justify-end gap-2">
            <Button variant="outline" type="button" onClick={onCancel}>
              {t("common.cancel")}
            </Button>
            <Button type="submit" disabled={isSubmitting}>
              {submitLabel}
            </Button>
          </div>
        )}
      </form>
    </Form>
  );
}
