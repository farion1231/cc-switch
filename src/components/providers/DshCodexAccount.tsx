import { useCallback, useState } from "react";
import { useForm } from "react-hook-form";
import { useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import { Button } from "@/components/ui/button";
import { Form } from "@/components/ui/form";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { fetchCodexOauthModels } from "@/lib/api/model-fetch";
import type { ProviderFormData } from "@/lib/schemas/provider";
import type { OpenCodeModel } from "@/types";
import { extractErrorMessage } from "@/utils/errorUtils";
import { BasicFormFields } from "./forms/BasicFormFields";
import { OpenCodeFormFields } from "./forms/OpenCodeFormFields";
import type { ProviderFormProps } from "./forms/ProviderForm";
import CodexOAuthSection from "./forms/CodexOAuthSection";

const noop = () => {};
type ModelConfig = { models: Record<string, OpenCodeModel> };

export function DshCodexAccount({
  initialData,
  onManageAccounts,
  onApplied,
}: {
  initialData?: ProviderFormProps["initialData"];
  onManageAccounts: () => void;
  onApplied: () => void;
}) {
  const { t } = useTranslation();
  const client = useQueryClient();
  const status = useQuery({
    queryKey: ["dsh-codex-account"],
    queryFn: () => invoke<string | null>("dsh_codex_account"),
  });
  const form = useForm<ProviderFormData>({
    defaultValues: {
      name: initialData?.name ?? "OpenAI Codex",
      notes: initialData?.notes ?? "",
      websiteUrl:
        initialData?.websiteUrl || "https://openai.com/chatgpt/pricing",
      icon: initialData?.icon || "openai",
      iconColor: initialData?.iconColor ?? "",
      settingsConfig: "{}",
    },
  });
  const [config, setConfig] = useState<ModelConfig>(() => ({
    models: (initialData?.settingsConfig?.models ??
      {}) as ModelConfig["models"],
  }));
  const [selected, setSelected] = useState<string | null | undefined>();
  const accountId = selected === undefined ? status.data : selected;
  const fetchModels = useCallback(async () => {
    if (!accountId) throw new Error(t("dsh.codex.selectAccount"));
    return fetchCodexOauthModels(accountId);
  }, [accountId, t]);
  const [confirm, setConfirm] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const name = form.watch("name");
  const ready = Boolean(accountId && name.trim() && !status.error);
  const apply = async () => {
    if (!ready || busy) return;
    setBusy(true);
    setError("");
    try {
      const identity = form.getValues();
      await invoke("dsh_codex_bind", {
        accountId,
        provider: {
          ...identity,
          id: "openai-codex",
          name: identity.name.trim(),
          category: "official",
          settingsConfig: { ...config, name: identity.name.trim() },
        },
      });
      await client.invalidateQueries();
      onApplied();
    } catch (e) {
      setError(extractErrorMessage(e));
    } finally {
      setBusy(false);
      setConfirm(false);
    }
  };
  return (
    <Form {...form}>
      <form
        id="provider-form"
        className="space-y-6"
        onSubmit={(event) => {
          event.preventDefault();
          if (ready && !busy) setConfirm(true);
        }}
      >
        <fieldset disabled={busy} className="min-w-0 space-y-6 border-0 p-0">
          <BasicFormFields form={form} />
          <p className="text-body text-fg-2">{t("dsh.codex.help")}</p>
          <CodexOAuthSection
            mode="select"
            selectedAccountId={accountId}
            onAccountSelect={setSelected}
            allowUnboundSelection={false}
            requireExplicitSelection
            onManageAccounts={onManageAccounts}
          />
          <OpenCodeFormFields
            modelsOnly
            allowBuiltinDefaults
            modelDialect="dsh"
            npm=""
            onNpmChange={noop}
            apiKey=""
            onApiKeyChange={noop}
            shouldShowApiKeyLink={false}
            websiteUrl=""
            baseUrl=""
            onBaseUrlChange={noop}
            headers={{}}
            onHeadersChange={noop}
            extraOptions={{}}
            onExtraOptionsChange={noop}
            fetchModels={fetchModels}
            models={config.models}
            onModelsChange={(models) => {
              setConfig({ models });
            }}
          />
        </fieldset>
        {status.error && (
          <p role="alert">{extractErrorMessage(status.error)}</p>
        )}
        {error && (
          <p role="alert" className="text-danger-text">
            {error}
          </p>
        )}
        <Button type="submit" disabled={!ready || busy || status.isPending}>
          {t("common.save")}
        </Button>
        <ConfirmDialog
          isOpen={confirm}
          onCancel={() => setConfirm(false)}
          pending={busy}
          variant="info"
          onConfirm={apply}
          title={t("common.save")}
          message={t("dsh.codex.confirm")}
        />
      </form>
    </Form>
  );
}
