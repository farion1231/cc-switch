import { useEffect, useState } from "react";
import { useForm } from "react-hook-form";
import { useTranslation } from "react-i18next";
import { Plus, Trash2 } from "lucide-react";
import { Form, FormLabel } from "@/components/ui/form";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { resolveManagedAccountId } from "@/lib/authBinding";
import { useProvidersQuery } from "@/lib/query/queries";
import type { ProviderFormData } from "@/lib/schemas/provider";
import { BasicFormFields } from "./BasicFormFields";
import { XaiOAuthSection } from "./XaiOAuthSection";
import type { ProviderFormProps } from "./ProviderForm";

const PROVIDER_KEY = "grok-oauth";

const DEFAULT_REASONING = {
  supportedEfforts: ["low", "medium", "high", "xhigh"],
  defaultEffort: "xhigh",
  canDisableThinking: false,
};

interface ModelRow {
  key: string;
  id: string;
  name?: string;
  maxInputTokens?: number;
  maxOutputTokens?: number;
  supportsToolCall?: boolean;
  supportsImages?: boolean;
  reasoning?: typeof DEFAULT_REASONING;
}

const newRow = (id = ""): ModelRow => ({ key: crypto.randomUUID(), id });

const defaultRows = (): ModelRow[] => [newRow("grok-4.7-build-fast")];

function rowsFrom(config: Record<string, unknown> | undefined): ModelRow[] {
  const models = config?.models;
  if (!Array.isArray(models)) return defaultRows();
  const rows = models.flatMap((model): ModelRow[] => {
    if (!model || typeof model !== "object") return [];
    const entry = model as Record<string, unknown>;
    const id = typeof entry.id === "string" ? entry.id : "";
    if (!id.trim()) return [];
    return [
      {
        key: crypto.randomUUID(),
        id,
        name: typeof entry.name === "string" ? entry.name : undefined,
        maxInputTokens:
          typeof entry.maxInputTokens === "number"
            ? entry.maxInputTokens
            : undefined,
        maxOutputTokens:
          typeof entry.maxOutputTokens === "number"
            ? entry.maxOutputTokens
            : undefined,
        supportsToolCall:
          typeof entry.supportsToolCall === "boolean"
            ? entry.supportsToolCall
            : undefined,
        supportsImages:
          typeof entry.supportsImages === "boolean"
            ? entry.supportsImages
            : undefined,
        reasoning:
          entry.reasoning && typeof entry.reasoning === "object"
            ? (entry.reasoning as ModelRow["reasoning"])
            : undefined,
      },
    ];
  });
  return rows.length > 0 ? rows : defaultRows();
}

function settingsFor(apiKey: string, rows: ModelRow[]) {
  return {
    vendor: "Grok OAuth",
    url: "http://127.0.0.1",
    apiKey,
    models: rows.map((row) => ({
      id: row.id.trim(),
      ...(row.name?.trim() ? { name: row.name.trim() } : {}),
      maxInputTokens: row.maxInputTokens ?? 500000,
      maxOutputTokens: row.maxOutputTokens ?? 8192,
      supportsToolCall: row.supportsToolCall ?? true,
      supportsImages: row.supportsImages ?? true,
      supportsReasoning: true,
      reasoning: row.reasoning ?? DEFAULT_REASONING,
    })),
  };
}

/**
 * Qoder CN 只有这一条：把已登录的 Grok OAuth 写成自定义供应商。
 * 模型 ID 由用户填写；本地密钥和网关地址在启用时由后端填进
 * ~/.qoder-cn/settings.json。
 */
export function QoderCnProviderForm({
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
  const [accountId, setAccountId] = useState<string | null>(
    () => resolveManagedAccountId(initialData?.meta, "xai_oauth") ?? null,
  );
  const [models, setModels] = useState<ModelRow[]>(() =>
    rowsFrom(initialData?.settingsConfig),
  );
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const { data: existing } = useProvidersQuery("qoder-cn");
  const keyTaken =
    !isEdit &&
    Object.prototype.hasOwnProperty.call(
      existing?.providers ?? {},
      PROVIDER_KEY,
    );
  const form = useForm<ProviderFormData>({
    defaultValues: {
      name: initialData?.name ?? "xAI (Grok) OAuth",
      notes: initialData?.notes ?? "",
      websiteUrl: initialData?.websiteUrl ?? "https://grok.com",
      icon: initialData?.icon ?? "grok",
      iconColor: initialData?.iconColor ?? "",
      settingsConfig: "{}",
    },
  });
  const name = form.watch("name");
  const ids = models.map((model) => model.id.trim());
  const modelsValid =
    ids.length > 0 &&
    ids.every((id) => id.startsWith("grok-")) &&
    new Set(ids).size === ids.length;
  const ready = Boolean(
    name.trim() && accountId && !keyTaken && modelsValid && !busy,
  );

  useEffect(() => {
    onSubmitReadyChange?.(ready);
  }, [ready, onSubmitReadyChange]);

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
            const savedKey =
              typeof initialData?.settingsConfig?.apiKey === "string"
                ? initialData.settingsConfig.apiKey
                : "";
            const apiKey =
              savedKey.length >= 32 ? savedKey : crypto.randomUUID();
            await onSubmit({
              ...identity,
              name: identity.name.trim(),
              websiteUrl: "https://grok.com",
              icon: identity.icon || "grok",
              meta: {
                ...initialData?.meta,
                providerType: "xai_oauth",
                authBinding: {
                  source: "managed_account",
                  authProvider: "xai_oauth",
                  accountId: accountId ?? undefined,
                },
              },
              providerKey: isEdit ? providerId : PROVIDER_KEY,
              presetCategory: "custom",
              settingsConfig: JSON.stringify(settingsFor(apiKey, models)),
            });
          } catch (submitError) {
            setError(String(submitError));
          } finally {
            setBusy(false);
            onSubmittingChange?.(false);
          }
        })}
      >
        <BasicFormFields form={form} />
        <XaiOAuthSection
          selectedAccountId={accountId}
          onAccountSelect={setAccountId}
        />
        <div className="space-y-2">
          <div className="flex items-center justify-between">
            <FormLabel>{t("opencode.models")}</FormLabel>
            <Button
              type="button"
              variant="outline"
              size="sm"
              className="h-7 gap-1"
              onClick={() => setModels((current) => [...current, newRow()])}
            >
              <Plus className="h-3.5 w-3.5" />
              {t("opencode.addModel")}
            </Button>
          </div>
          <p className="text-sm text-fg-2">{t("appsPage.qoderCnModelsHint")}</p>
          {models.map((model, index) => {
            const id = model.id.trim();
            const invalid = id !== "" && !id.startsWith("grok-");
            const duplicate =
              id !== "" && ids.filter((item) => item === id).length > 1;
            return (
              <div key={model.key} className="flex gap-2">
                <Input
                  value={model.id}
                  onChange={(event) =>
                    setModels((current) =>
                      current.map((row) =>
                        row.key === model.key
                          ? { ...row, id: event.target.value }
                          : row,
                      ),
                    )
                  }
                  placeholder="grok-4.7-build-fast"
                  autoComplete="off"
                  aria-invalid={invalid || duplicate || undefined}
                  className={invalid || duplicate ? "border-destructive" : ""}
                />
                <Button
                  type="button"
                  variant="ghost"
                  size="icon"
                  disabled={models.length === 1}
                  onClick={() =>
                    setModels((current) =>
                      current.filter((_, rowIndex) => rowIndex !== index),
                    )
                  }
                  aria-label={t("common.delete")}
                >
                  <Trash2 className="h-4 w-4" />
                </Button>
              </div>
            );
          })}
          {!modelsValid ? (
            <p className="text-sm text-destructive">
              {t("appsPage.qoderCnModelsHint")}
            </p>
          ) : null}
        </div>
        <p className="text-sm text-fg-2">{t("workbuddy.form.grokOAuthHint")}</p>
        <p className="text-sm text-fg-2">{t("appsPage.qoderCnHint")}</p>
        {keyTaken ? (
          <p className="text-sm text-destructive">
            {t("appsPage.qoderCnAlreadyAdded")}
          </p>
        ) : null}
        {error ? <p className="text-sm text-destructive">{error}</p> : null}
        {showButtons ? (
          <div className="flex justify-end gap-2">
            <Button type="button" variant="outline" onClick={onCancel}>
              {t("common.cancel")}
            </Button>
            <Button type="submit" disabled={!ready}>
              {submitLabel}
            </Button>
          </div>
        ) : null}
      </form>
    </Form>
  );
}
