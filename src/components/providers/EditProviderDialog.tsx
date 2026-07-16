import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Save } from "lucide-react";
import { Button } from "@/components/ui/button";
import { FullScreenPanel } from "@/components/common/FullScreenPanel";
import type { Provider } from "@/types";
import {
  ProviderForm,
  type ProviderFormValues,
} from "@/components/providers/forms/ProviderForm";
import type { AppId } from "@/lib/api";
import { providerSecurityApi } from "@/lib/api/providerSecurity";
import { ProviderCredentialConflict } from "@/components/providers/ProviderCredentialConflict";
import type { ProviderSecurityStatus } from "@/types/providerSecurity";
import { extractErrorMessage } from "@/utils/errorUtils";

interface EditProviderDialogProps {
  open: boolean;
  provider: Provider | null;
  onOpenChange: (open: boolean) => void;
  onSubmit: (payload: {
    provider: Provider;
    originalId?: string;
  }) => Promise<void> | void;
  appId: AppId;
  isProxyTakeover?: boolean; // 代理接管模式下不读取 live（避免显示被接管后的代理配置）
}

export function EditProviderDialog({
  open,
  provider,
  onOpenChange,
  onSubmit,
  appId,
  isProxyTakeover = false,
}: EditProviderDialogProps) {
  const { t } = useTranslation();
  const [isFormSubmitting, setIsFormSubmitting] = useState(false);
  const [securityStatus, setSecurityStatus] =
    useState<ProviderSecurityStatus | null>(null);
  const [saveError, setSaveError] = useState<string | null>(null);

  // Load DB-vs-Live credential conflict status (does not overwrite form fields).
  useEffect(() => {
    if (!open || !provider?.id) {
      setSecurityStatus(null);
      setSaveError(null);
      return;
    }
    let cancelled = false;
    void (async () => {
      try {
        const status = await providerSecurityApi.status(appId, provider.id);
        if (!cancelled) setSecurityStatus(status);
      } catch {
        if (!cancelled) setSecurityStatus(null);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [open, provider?.id, appId]);
  const initialSettingsConfig = useMemo(
    () => (provider?.settingsConfig ?? {}) as Record<string, unknown>,
    [provider?.settingsConfig],
  );

  // 固定 initialData，防止 provider 对象更新时重置表单
  const initialData = useMemo(() => {
    if (!provider) return null;
    return {
      name: provider.name,
      notes: provider.notes,
      websiteUrl: provider.websiteUrl,
      settingsConfig: initialSettingsConfig,
      category: provider.category,
      meta: provider.meta,
      icon: provider.icon,
      iconColor: provider.iconColor,
    };
  }, [
    open, // 修复：编辑保存后再次打开显示旧数据，依赖 open 确保每次打开时重新读取最新 provider 数据
    provider?.id, // 只依赖 ID，provider 对象更新不会触发重新计算
    provider?.meta, // 供应商元数据变化时重新初始化表单
    initialSettingsConfig,
  ]);

  const handleSubmit = useCallback(
    async (values: ProviderFormValues) => {
      if (!provider) return;

      // 注意：values.settingsConfig 已经是最终的配置字符串
      // ProviderForm 已经为不同的 app 类型（Claude/Codex/Gemini）正确组装了配置
      const parsedConfig = JSON.parse(values.settingsConfig) as Record<
        string,
        unknown
      >;
      const nextProviderId =
        (appId === "opencode" || appId === "openclaw") &&
        values.providerKey?.trim()
          ? values.providerKey.trim()
          : provider.id;

      const updatedProvider: Provider = {
        ...provider,
        id: nextProviderId,
        name: values.name.trim(),
        notes: values.notes?.trim() || undefined,
        websiteUrl: values.websiteUrl?.trim() || undefined,
        settingsConfig: parsedConfig,
        icon: values.icon?.trim() || undefined,
        iconColor: values.iconColor?.trim() || undefined,
        ...(values.presetCategory ? { category: values.presetCategory } : {}),
        // 保留或更新 meta 字段
        ...(values.meta ? { meta: values.meta } : {}),
      };

      // Preserve optimistic-concurrency revision from list/query.
      const withRevision: Provider = {
        ...updatedProvider,
        revision: provider.revision,
      };

      try {
        await onSubmit({
          provider: withRevision,
          originalId: provider.id,
        });
        onOpenChange(false);
      } catch (error) {
        const detail = extractErrorMessage(error) || String(error ?? "");
        setSaveError(detail);
        if (
          detail.includes("provider_revision_conflict") ||
          detail.includes("revision")
        ) {
          // Refresh security status so user can re-resolve.
          try {
            const status = await providerSecurityApi.status(appId, provider.id);
            setSecurityStatus(status);
          } catch {
            /* ignore */
          }
        }
        throw error;
      }
    },
    [appId, onSubmit, onOpenChange, provider],
  );

  if (!provider || !initialData) {
    return null;
  }

  return (
    <FullScreenPanel
      isOpen={open}
      title={t("provider.editProvider")}
      onClose={() => onOpenChange(false)}
      footer={
        <Button
          type="submit"
          form="provider-form"
          disabled={isFormSubmitting}
          className="bg-primary text-primary-foreground hover:bg-primary/90"
        >
          <Save className="h-4 w-4 mr-2" />
          {t("common.save")}
        </Button>
      }
    >
      <div className="space-y-4">
        {securityStatus && securityStatus.conflicts.length > 0 ? (
          <ProviderCredentialConflict
            appId={appId}
            providerId={provider.id}
            revision={securityStatus.revision}
            conflicts={securityStatus.conflicts}
            onImported={async () => {
              try {
                const status = await providerSecurityApi.status(
                  appId,
                  provider.id,
                );
                setSecurityStatus(status);
              } catch {
                /* ignore */
              }
            }}
          />
        ) : null}
        {saveError ? (
          <div className="rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive">
            {saveError}
          </div>
        ) : null}
        <ProviderForm
          appId={appId}
          providerId={provider.id}
          submitLabel={t("common.save")}
          onSubmit={handleSubmit}
          onCancel={() => onOpenChange(false)}
          onSubmittingChange={setIsFormSubmitting}
          initialData={initialData}
          showButtons={false}
          isProxyTakeover={isProxyTakeover}
        />
      </div>
    </FullScreenPanel>
  );
}
