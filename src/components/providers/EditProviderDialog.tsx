import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Save } from "lucide-react";
import { Button } from "@/components/ui/button";
import { FullScreenPanel } from "@/components/common/FullScreenPanel";
import type { Provider } from "@/types";
import {
  ProviderForm,
  type ProviderFormValues,
} from "@/components/providers/forms/ProviderForm";
import { AuthSettingsPanel } from "@/components/providers/AuthSettingsPanel";
import {
  openclawApi,
  providersApi,
  vscodeApi,
  type AppId,
  type ManagedAuthProvider,
} from "@/lib/api";
import type {
  EditorConflictPolicy,
  ProviderEditorSave,
  ProviderEditorView,
} from "@/lib/api/providers";
import { useLiveEditConflict } from "@/components/providers/LiveEditConflictDialog";
import { toastEditorViewFailed } from "@/components/providers/forms/hooks/useDraftEditorProjection";
import { usesEditorView } from "@/config/appConfig";

interface EditProviderDialogProps {
  open: boolean;
  provider: Provider | null;
  onOpenChange: (open: boolean) => void;
  onSubmit: (payload: {
    provider: Provider;
    originalId?: string;
    editorSave?: ProviderEditorSave;
  }) => Promise<void> | void;
  appId: AppId;
  isProxyTakeover?: boolean; // 代理接管模式下不读取 live（避免显示被接管后的代理配置）
}

const asRecord = (value: unknown): Record<string, unknown> | null =>
  typeof value === "object" && value !== null && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;

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
  const [authSettingsTarget, setAuthSettingsTarget] =
    useState<ManagedAuthProvider | null>(null);

  useEffect(() => {
    setAuthSettingsTarget(null);
  }, [appId, open, provider?.id]);

  const formReadyToken = useMemo(
    () => Symbol("provider-form-ready"),
    [appId, open, provider?.id],
  );
  const currentFormReadyToken = useRef(formReadyToken);
  currentFormReadyToken.current = formReadyToken;
  const [formReadyState, setFormReadyState] = useState({
    token: formReadyToken,
    ready: appId !== "pi",
  });
  const isFormReady =
    formReadyState.token === formReadyToken
      ? formReadyState.ready
      : appId !== "pi";
  const handleSubmitReadyChange = useCallback(
    (ready: boolean) => {
      if (currentFormReadyToken.current === formReadyToken) {
        setFormReadyState({ token: formReadyToken, ready });
      }
    },
    [formReadyToken],
  );

  // 默认使用传入的 provider.settingsConfig，若当前编辑对象是"当前生效供应商"，则尝试读取实时配置替换初始值
  const [liveSettings, setLiveSettings] = useState<Record<
    string,
    unknown
  > | null>(null);

  // 使用 ref 标记是否已经加载过，防止重复读取覆盖用户编辑
  const [hasLoadedLive, setHasLoadedLive] = useState(false);

  // Claude：底部 JSON 显示「切到这个供应商之后 settings.json 的样子」，保存时拿它做三方比较。
  const [editorView, setEditorView] = useState<ProviderEditorView | null>(null);
  const { submitWithConflictRetry, conflictDialog } = useLiveEditConflict();

  const closeDialog = useCallback(() => {
    setAuthSettingsTarget(null);
    onOpenChange(false);
  }, [onOpenChange]);

  const handlePanelClose = useCallback(() => {
    if (authSettingsTarget) {
      setAuthSettingsTarget(null);
      return;
    }
    closeDialog();
  }, [authSettingsTarget, closeDialog]);

  useEffect(() => {
    let cancelled = false;
    const load = async () => {
      if (!open || !provider) {
        setLiveSettings(null);
        setEditorView(null);
        setHasLoadedLive(false);
        return;
      }

      // 关键修复：只在首次打开时加载一次
      if (hasLoadedLive) {
        return;
      }

      // 切换式应用：编辑任何供应商都显示切换投影（关键字段、独有字段来自这一行，其余
      // 来自 live），代理模式下也一样，关键字段显示的是这个供应商自己的值。
      if (usesEditorView(appId)) {
        try {
          const view = await providersApi.getEditorView(
            appId,
            asRecord(provider.settingsConfig) ?? {},
            provider.category,
            provider.id,
          );
          if (!cancelled) {
            setEditorView(view);
            setLiveSettings(view.settings);
          }
        } catch (error) {
          // 读不了配置文件（比如手改坏了）：退回显示保存的供应商配置。
          if (!cancelled) {
            setEditorView(null);
            setLiveSettings(null);
            toastEditorViewFailed(t, error);
          }
        } finally {
          if (!cancelled) {
            setHasLoadedLive(true);
          }
        }
        return;
      }

      // 代理接管模式：Live 配置已被代理改写，读取 live 会导致编辑界面展示代理地址/占位符等内容
      // 因此直接回退到 SSOT（数据库）配置，避免用户困惑与误保存
      if (isProxyTakeover) {
        if (!cancelled) {
          setLiveSettings(null);
          setHasLoadedLive(true);
        }
        return;
      }

      // OpenCode uses additive mode, while Pi's shared models.json is owned by
      // the catalog coordinator. Neither has a per-provider generic live
      // snapshot that may replace the DB aggregate in this form.
      if (appId === "opencode" || appId === "pi" || appId === "mcode") {
        if (!cancelled) {
          setLiveSettings(null);
          setHasLoadedLive(true);
        }
        return;
      }

      if (appId === "openclaw") {
        try {
          const live = await openclawApi.getLiveProvider(provider.id);
          if (!cancelled && live && typeof live === "object") {
            setLiveSettings(live);
          } else if (!cancelled) {
            setLiveSettings(null);
          }
        } catch {
          if (!cancelled) {
            setLiveSettings(null);
          }
        } finally {
          if (!cancelled) {
            setHasLoadedLive(true);
          }
        }
        return;
      }

      try {
        const currentId = await providersApi.getCurrent(appId);
        if (currentId && provider.id === currentId) {
          try {
            const live = (await vscodeApi.getLiveProviderSettings(
              appId,
            )) as Record<string, unknown>;
            if (!cancelled && live && typeof live === "object") {
              setLiveSettings(live);
              setHasLoadedLive(true);
            }
          } catch {
            // 读取实时配置失败则回退到 SSOT（不打断编辑流程）
            if (!cancelled) {
              setLiveSettings(null);
              setHasLoadedLive(true);
            }
          }
        } else {
          if (!cancelled) {
            setLiveSettings(null);
            setHasLoadedLive(true);
          }
        }
      } finally {
        // no-op
      }
    };
    void load();
    return () => {
      cancelled = true;
    };
  }, [open, provider?.id, appId, hasLoadedLive, isProxyTakeover]); // 只依赖 provider.id，不依赖整个 provider 对象

  const initialSettingsConfig = useMemo(
    () => liveSettings ?? asRecord(provider?.settingsConfig) ?? {},
    [liveSettings, provider?.settingsConfig],
  ); // 只依赖表单初始化所需字段，不依赖整个 provider

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
        (appId === "opencode" || appId === "openclaw" || appId === "pi") &&
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

      const submit = async (onConflict: EditorConflictPolicy) => {
        await onSubmit({
          provider: updatedProvider,
          originalId: provider.id,
          ...(editorView
            ? { editorSave: { base: editorView.settings, onConflict } }
            : {}),
        });
        closeDialog();
      };
      await submitWithConflictRetry(submit);
    },
    [
      appId,
      onSubmit,
      closeDialog,
      provider,
      editorView,
      submitWithConflictRetry,
    ],
  );

  if (!provider || !initialData) {
    return null;
  }

  const waitingForEditorView = usesEditorView(appId) && !hasLoadedLive;

  return (
    <FullScreenPanel
      isOpen={open}
      title={t("provider.editProvider")}
      onClose={handlePanelClose}
      contentClassName={appId === "pi" ? "pb-0" : undefined}
      footer={
        <Button
          type="submit"
          form="provider-form"
          disabled={isFormSubmitting || !isFormReady}
          className="bg-primary text-primary-foreground hover:bg-primary/90"
        >
          <Save className="h-4 w-4 mr-2" />
          {t("common.save")}
        </Button>
      }
    >
      {waitingForEditorView ? (
        <div className="py-12 text-center text-sm text-muted-foreground">
          {t("common.loading")}
        </div>
      ) : (
        <ProviderForm
          appId={appId}
          providerId={provider.id}
          submitLabel={t("common.save")}
          onSubmit={handleSubmit}
          onCancel={closeDialog}
          onManageAuthAccounts={setAuthSettingsTarget}
          onSubmittingChange={setIsFormSubmitting}
          onSubmitReadyChange={handleSubmitReadyChange}
          initialData={initialData}
          showButtons={false}
          isProxyTakeover={isProxyTakeover}
          inactiveFields={editorView?.inactive}
        />
      )}
      {conflictDialog}
      <AuthSettingsPanel
        target={authSettingsTarget}
        onClose={() => setAuthSettingsTarget(null)}
      />
    </FullScreenPanel>
  );
}
