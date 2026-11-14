import { useEffect, useState } from "react";
import { useForm } from "react-hook-form";
import { useTranslation } from "react-i18next";
import { Plus, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Form } from "@/components/ui/form";
import { ImeSafeInput } from "@/components/ui/ime-safe-input";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import type { ProviderFormData } from "@/lib/schemas/provider";
import { useProvidersQuery } from "@/lib/query/queries";
import { BasicFormFields, REQUIRED_LABEL } from "./BasicFormFields";
import { ApiKeySection, EndpointField } from "./shared";
import type { ProviderFormProps } from "./ProviderForm";

/** 原生文件支持的三种接口；空字符串表示不写 `api`，交给客户端按模型推断。 */
const API_OPTIONS = [
  "openai-completions",
  "openai-responses",
  "anthropic-messages",
] as const;
const API_AUTO = "auto";
const DEFAULT_API_KEY_ENV = "LABUNBUN_API_KEY";
const KEY_PATTERN = /^[a-z0-9]+(-[a-z0-9]+)*$/;
const normalizeKey = (value: string) =>
  value.toLowerCase().replace(/[^a-z0-9-]/g, "");

interface ModelRow {
  id: string;
  contextWindow: string;
  maxOutputTokens: string;
}

const toModelRows = (models: unknown): ModelRow[] => {
  if (!Array.isArray(models)) return [];
  return models.map((model) => {
    const row = (model ?? {}) as Record<string, unknown>;
    const text = (value: unknown) =>
      typeof value === "number" && Number.isFinite(value) ? String(value) : "";
    return {
      id: typeof row.id === "string" ? row.id : "",
      contextWindow: text(row.contextWindow),
      maxOutputTokens: text(row.maxOutputTokens),
    };
  });
};

const positiveInteger = (value: string): boolean => {
  const trimmed = value.trim();
  return /^\d+$/.test(trimmed) && Number(trimmed) > 0;
};

const emptyModelRow = (): ModelRow => ({
  id: "",
  contextWindow: "128000",
  maxOutputTokens: "8192",
});

/**
 * LaBunbun Code 的供应商表单。卡片保存 `baseUrl`、`apiKey`、可选的 `api`/`apiKeyEnv` 和
 * `models`；切换时后端把这些写成 `~/.labunbun/settings.json` 里的一条原生供应商。
 */
export function LaBunbunProviderForm({
  providerId,
  submitLabel,
  onSubmit,
  onCancel,
  onSubmittingChange,
  onSubmitReadyChange,
  initialData,
  showButtons = true,
  appId,
}: ProviderFormProps) {
  const { t } = useTranslation();
  const isEdit = Boolean(initialData);
  const initialConfig = (initialData?.settingsConfig ?? {}) as Record<
    string,
    unknown
  >;
  const [providerKey, setProviderKey] = useState(providerId ?? "");
  const [baseUrl, setBaseUrl] = useState(
    typeof initialConfig.baseUrl === "string" ? initialConfig.baseUrl : "",
  );
  const [apiKey, setApiKey] = useState(
    typeof initialConfig.apiKey === "string" ? initialConfig.apiKey : "",
  );
  const [api, setApi] = useState(
    typeof initialConfig.api === "string" ? initialConfig.api : API_AUTO,
  );
  const [apiKeyEnv, setApiKeyEnv] = useState(
    typeof initialConfig.apiKeyEnv === "string"
      ? initialConfig.apiKeyEnv
      : DEFAULT_API_KEY_ENV,
  );
  const [models, setModels] = useState<ModelRow[]>(() => {
    const rows = toModelRows(initialConfig.models);
    return rows.length ? rows : [emptyModelRow()];
  });
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  const form = useForm<ProviderFormData>({
    defaultValues: {
      name: initialData?.name ?? "",
      notes: initialData?.notes ?? "",
      websiteUrl: initialData?.websiteUrl ?? "",
      icon: initialData?.icon ?? "",
      iconColor: initialData?.iconColor ?? "",
      settingsConfig: JSON.stringify(initialConfig),
    },
  });
  const name = form.watch("name");

  const { data: existing } = useProvidersQuery(appId);
  const keyTaken =
    !isEdit &&
    providerKey !== "" &&
    Object.prototype.hasOwnProperty.call(
      existing?.providers ?? {},
      providerKey,
    );
  const keyInvalid =
    !isEdit && providerKey !== "" && !KEY_PATTERN.test(providerKey);

  const baseUrlValid =
    baseUrl.trim().startsWith("http://") ||
    baseUrl.trim().startsWith("https://");
  const modelsValid =
    models.length > 0 &&
    models.every(
      (model) =>
        model.id.trim() !== "" &&
        positiveInteger(model.contextWindow) &&
        positiveInteger(model.maxOutputTokens),
    );
  const ready = Boolean(
    name.trim() &&
      baseUrlValid &&
      apiKey.trim() &&
      modelsValid &&
      (isEdit || (providerKey && !keyInvalid && !keyTaken)),
  );

  useEffect(() => {
    onSubmitReadyChange?.(ready);
  }, [ready, onSubmitReadyChange]);

  const updateModel = (index: number, patch: Partial<ModelRow>) => {
    setModels((current) =>
      current.map((model, i) => (i === index ? { ...model, ...patch } : model)),
    );
  };
  const removeModel = (index: number) => {
    setModels((current) => current.filter((_, i) => i !== index));
  };
  const addModel = () => {
    setModels((current) => [...current, emptyModelRow()]);
  };

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
            await onSubmit({
              ...identity,
              name: identity.name.trim(),
              meta: initialData?.meta,
              providerKey: isEdit ? providerId : providerKey,
              presetCategory: "custom",
              settingsConfig: JSON.stringify({
                baseUrl: baseUrl.trim(),
                apiKey: apiKey.trim(),
                apiKeyEnv: apiKeyEnv.trim() || DEFAULT_API_KEY_ENV,
                ...(api === API_AUTO ? {} : { api }),
                models: models.map((model) => ({
                  id: model.id.trim(),
                  contextWindow: Number(model.contextWindow.trim()),
                  maxOutputTokens: Number(model.maxOutputTokens.trim()),
                })),
              }),
            });
          } catch (caught) {
            setError(String(caught));
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
          disabled={busy}
          className="min-w-0 space-y-6 border-0 p-0 disabled:opacity-50"
        >
          <BasicFormFields
            form={form}
            beforeNameSlot={
              <div className="space-y-2">
                <Label htmlFor="labunbun-provider-key">
                  {t("labunbun.providerKey")}
                  <span aria-hidden="true" className="text-destructive ml-1">
                    *
                  </span>
                </Label>
                <ImeSafeInput
                  id="labunbun-provider-key"
                  value={providerKey}
                  onValueChange={setProviderKey}
                  normalize={normalizeKey}
                  disabled={isEdit}
                  placeholder={t("labunbun.providerKeyPlaceholder")}
                  autoComplete="off"
                  className={
                    keyTaken || keyInvalid ? "border-destructive" : undefined
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
                    ? t("labunbun.providerKeyDuplicate")
                    : keyInvalid
                      ? t("labunbun.providerKeyInvalid")
                      : isEdit
                        ? t("labunbun.providerKeyLockedHint")
                        : t("labunbun.providerKeyHint")}
                </p>
              </div>
            }
          />

          <EndpointField
            id="labunbun-base-url"
            label={t("labunbun.baseUrl")}
            value={baseUrl}
            onChange={setBaseUrl}
            placeholder={t("labunbun.baseUrlPlaceholder")}
            showManageButton={false}
          />

          <ApiKeySection
            id="labunbun-api-key"
            value={apiKey}
            onChange={setApiKey}
            shouldShowLink={false}
            websiteUrl=""
            required
          />

          <div className="space-y-2">
            <Label htmlFor="labunbun-api">{t("labunbun.apiFormat")}</Label>
            <Select value={api} onValueChange={setApi}>
              <SelectTrigger id="labunbun-api">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value={API_AUTO}>
                  {t("labunbun.apiAuto")}
                </SelectItem>
                {API_OPTIONS.map((option) => (
                  <SelectItem key={option} value={option}>
                    {option}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <p className="text-xs text-fg-2">{t("labunbun.apiHint")}</p>
          </div>

          <div className="space-y-2">
            <Label htmlFor="labunbun-api-key-env">
              {t("labunbun.apiKeyEnv")}
            </Label>
            <Input
              id="labunbun-api-key-env"
              value={apiKeyEnv}
              onChange={(event) => setApiKeyEnv(event.target.value)}
              placeholder={DEFAULT_API_KEY_ENV}
              autoComplete="off"
            />
            <p className="text-xs text-fg-2">{t("labunbun.apiKeyEnvHint")}</p>
          </div>

          <div className="space-y-3">
            <div className="flex items-center justify-between">
              <Label className={REQUIRED_LABEL}>{t("labunbun.models")}</Label>
              <Button
                type="button"
                variant="neutral"
                size="sm"
                onClick={addModel}
              >
                <Plus className="mr-1 h-4 w-4" />
                {t("labunbun.addModel")}
              </Button>
            </div>
            {models.map((model, index) => (
              <div
                key={index}
                className="grid grid-cols-1 gap-2 rounded-lg border border-border p-3 sm:grid-cols-[minmax(0,2fr)_minmax(0,1fr)_minmax(0,1fr)_auto]"
              >
                <Input
                  aria-label={t("labunbun.modelId")}
                  value={model.id}
                  onChange={(event) =>
                    updateModel(index, { id: event.target.value })
                  }
                  placeholder={t("labunbun.modelIdPlaceholder")}
                  autoComplete="off"
                />
                <Input
                  aria-label={t("labunbun.contextWindow")}
                  value={model.contextWindow}
                  onChange={(event) =>
                    updateModel(index, { contextWindow: event.target.value })
                  }
                  placeholder={t("labunbun.contextWindow")}
                  inputMode="numeric"
                />
                <Input
                  aria-label={t("labunbun.maxOutputTokens")}
                  value={model.maxOutputTokens}
                  onChange={(event) =>
                    updateModel(index, { maxOutputTokens: event.target.value })
                  }
                  placeholder={t("labunbun.maxOutputTokens")}
                  inputMode="numeric"
                />
                <Button
                  type="button"
                  variant="quiet"
                  size="icon"
                  aria-label={t("labunbun.removeModel")}
                  disabled={models.length === 1}
                  onClick={() => removeModel(index)}
                >
                  <Trash2 className="h-4 w-4" />
                </Button>
              </div>
            ))}
            <p className="text-xs text-fg-2">{t("labunbun.modelsHint")}</p>
          </div>
        </fieldset>
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
