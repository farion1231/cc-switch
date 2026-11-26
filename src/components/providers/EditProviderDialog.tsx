import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Loader2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Notice } from "@/components/ui/notice";
import { APP_DISPLAY_NAME } from "@/components/shell/AppGlyph";
import { FullScreenPanel } from "@/components/common/FullScreenPanel";
import type { Provider } from "@/types";
import type { AppMode } from "@/types/proxy";
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
  type AuthCenterTarget,
} from "@/lib/api";
import type {
  EditorConflictPolicy,
  ProviderEditorSave,
  ProviderEditorView,
} from "@/lib/api/providers";
import { useLiveEditConflict } from "@/components/providers/LiveEditConflictDialog";
import { toastEditorViewFailed } from "@/components/providers/forms/hooks/useDraftEditorProjection";
import { appHas } from "@/config/appSpecs";

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
  /** 正在编辑的是当前生效的那家（切换式应用）：页头下提示保存后立即生效 */
  isCurrent?: boolean;
  /** 从供应商页哪一格打开的，见 ProviderForm 的同名参数 */
  modeView?: AppMode;
}

/** 直连时保存当前供应商会写进的配置文件 */
const LIVE_FILE: Partial<Record<AppId, string>> = {
  claude: "~/.claude/settings.json",
  codex: "~/.codex/config.toml",
  gemini: "~/.gemini/.env",
};
const SWITCH_APPS: AppId[] = [
  "claude",
  "codex",
  "gemini",
  "grokbuild",
  "claude-desktop",
];

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
  isCurrent = false,
  modeView,
}: EditProviderDialogProps) {
  const { t } = useTranslation();
  const [authSettingsTarget, setAuthSettingsTarget] =
    useState<AuthCenterTarget | null>(null);

  useEffect(() => {
    setAuthSettingsTarget(null);
  }, [appId, open, provider?.id]);

  const formSessionSequence = useRef(0);
  const formSessionToken = useMemo(
    () => ++formSessionSequence.current,
    [appId, open, provider?.id],
  );
  const currentFormSessionToken = useRef(formSessionToken);
  currentFormSessionToken.current = formSessionToken;
  // 表单用了聚合的简化布局：页头应用名后标「聚合模式」
  const [stackLayoutState, setStackLayoutState] = useState({
    token: formSessionToken,
    stackLayout: false,
  });
  const stackLayout =
    stackLayoutState.token === formSessionToken && stackLayoutState.stackLayout;
  const handleStackLayoutChange = useCallback(
    (next: boolean) => {
      if (currentFormSessionToken.current === formSessionToken) {
        setStackLayoutState({ token: formSessionToken, stackLayout: next });
      }
    },
    [formSessionToken],
  );
  // The host stays mounted between edits. A pending save from a closed form
  // must neither disable the next editor nor clear its own pending submission.
  const [formSubmittingState, setFormSubmittingState] = useState({
    token: formSessionToken,
    submitting: false,
  });
  const isFormSubmitting =
    formSubmittingState.token === formSessionToken &&
    formSubmittingState.submitting;
  const handleSubmittingChange = useCallback(
    (submitting: boolean) => {
      if (currentFormSessionToken.current === formSessionToken) {
        setFormSubmittingState({ token: formSessionToken, submitting });
      }
    },
    [formSessionToken],
  );
  const [formReadyState, setFormReadyState] = useState({
    token: formSessionToken,
    ready: !appHas(appId, "piFamily"),
  });
  const isFormReady =
    formReadyState.token === formSessionToken
      ? formReadyState.ready
      : !appHas(appId, "piFamily");
  const handleSubmitReadyChange = useCallback(
    (ready: boolean) => {
      if (currentFormSessionToken.current === formSessionToken) {
        setFormReadyState({ token: formSessionToken, ready });
      }
    },
    [formSessionToken],
  );

  // 默认使用传入的 provider.settingsConfig，若当前编辑对象是"当前生效供应商"，则尝试读取实时配置替换初始值
  // 与 formSessionToken 绑定，避免同窗口切换供应商或应用时残留上一个供应商的 liveSettings / editorView。
  const [liveState, setLiveState] = useState<{
    token: number;
    loaded: boolean;
    settings: Record<string, unknown> | null;
    editorView: ProviderEditorView | null;
  }>({
    token: formSessionToken,
    loaded: false,
    settings: null,
    editorView: null,
  });
  const isCurrentLiveState = liveState.token === formSessionToken;
  const hasLoadedLive = isCurrentLiveState && liveState.loaded;
  const liveSettings = isCurrentLiveState ? liveState.settings : null;
  const editorView = isCurrentLiveState ? liveState.editorView : null;
  const loadedSessionTokenRef = useRef<number | null>(null);
  const tRef = useRef(t);
  tRef.current = t;
  const isProxyTakeoverRef = useRef(isProxyTakeover);
  isProxyTakeoverRef.current = isProxyTakeover;

  const { submitWithConflictRetry, conflictDialog } =
    useLiveEditConflict(formSessionToken);

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
    const commitLiveState = (
      loaded: boolean,
      settings: Record<string, unknown> | null,
      view: ProviderEditorView | null,
    ) => {
      if (!cancelled && currentFormSessionToken.current === formSessionToken) {
        if (loaded) {
          loadedSessionTokenRef.current = formSessionToken;
        }
        setLiveState({
          token: formSessionToken,
          loaded,
          settings,
          editorView: view,
        });
      }
    };
    const load = async () => {
      if (!open || !provider) {
        loadedSessionTokenRef.current = null;
        commitLiveState(false, null, null);
        return;
      }

      // 关键修复：同一编辑会话只加载一次，避免编辑期间轮询或重渲染覆盖表单与三方基准
      if (loadedSessionTokenRef.current === formSessionToken) {
        return;
      }

      // 切换式应用：编辑任何供应商都显示切换投影（关键字段、独有字段来自这一行，其余
      // 来自 live），代理模式下也一样，关键字段显示的是这个供应商自己的值。
      if (appHas(appId, "editorView")) {
        try {
          const view = await providersApi.getEditorView(
            appId,
            asRecord(provider.settingsConfig) ?? {},
            provider.category,
            provider.id,
            appId === "codex" ? provider.meta : undefined,
          );
          commitLiveState(true, view.settings, view);
        } catch (error) {
          // 读不了配置文件（比如手改坏了）：退回显示保存的供应商配置。
          commitLiveState(true, null, null);
          if (
            !cancelled &&
            currentFormSessionToken.current === formSessionToken
          ) {
            toastEditorViewFailed(tRef.current, error);
          }
        }
        return;
      }

      // 代理接管模式：Live 配置已被代理改写，读取 live 会导致编辑界面展示代理地址/占位符等内容
      // 因此直接回退到 SSOT（数据库）配置，避免用户困惑与误保存
      if (isProxyTakeoverRef.current) {
        commitLiveState(true, null, null);
        return;
      }

      // OpenCode uses additive mode, while Pi's (and Oh My Pi's) native models
      // file is owned by its provider service. Neither has a per-provider generic
      // live snapshot that may replace the DB aggregate in this form.
      if (
        appId === "opencode" ||
        appId === "pi" ||
        appId === "mcode" ||
        appId === "kimi" ||
        appId === "dsh" ||
        appId === "omp" ||
        appId === "copilot" ||
        appId === "workbuddy" ||
        appId === "codebuddy"
      ) {
        commitLiveState(true, null, null);
        return;
      }

      if (appId === "openclaw") {
        try {
          const live = await openclawApi.getLiveProvider(provider.id);
          commitLiveState(
            true,
            live && typeof live === "object" ? live : null,
            null,
          );
        } catch {
          commitLiveState(true, null, null);
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
            commitLiveState(
              true,
              live && typeof live === "object" ? live : null,
              null,
            );
          } catch {
            // 读取实时配置失败则回退到 SSOT（不打断编辑流程）
            commitLiveState(true, null, null);
          }
        } else {
          commitLiveState(true, null, null);
        }
      } catch {
        commitLiveState(true, null, null);
      }
    };
    void load();
    return () => {
      cancelled = true;
    };
  }, [open, provider?.id, appId, formSessionToken]);

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
        if (currentFormSessionToken.current === formSessionToken) closeDialog();
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
      formSessionToken,
    ],
  );

  if (!provider || !initialData) {
    return null;
  }

  const waitingForEditorView = appHas(appId, "editorView") && !hasLoadedLive;

  const liveFile = !isProxyTakeover ? LIVE_FILE[appId] : undefined;
  const currentNotice =
    isCurrent && SWITCH_APPS.includes(appId) ? (
      <Notice
        tone="neutral"
        title={
          liveFile ? (
            <>
              {t("provider.editCurrentNoticeFile")}{" "}
              <code className="font-mono text-caption">{liveFile}</code>
              {t("provider.editCurrentNoticeFileEnd")}
            </>
          ) : (
            t("provider.editCurrentNotice")
          )
        }
      />
    ) : null;

  return (
    <FullScreenPanel
      // Keep the list visible until the initial projection is ready.
      // Showing a loading page first causes a flash for fast local reads.
      isOpen={open && !waitingForEditorView}
      trackUnsavedChanges
      title={t("provider.editProviderNamed", { name: provider.name })}
      subtitle={
        stackLayout
          ? t("provider.formSubtitleStack", {
              app: APP_DISPLAY_NAME[appId],
              defaultValue: "{{app}}（聚合模式）",
            })
          : APP_DISPLAY_NAME[appId]
      }
      backLabel={t("provider.backToList")}
      onClose={handlePanelClose}
      contentClassName={appId === "pi" ? "pb-0 pt-4" : "pt-4"}
      footer={
        <>
          <Button
            type="button"
            variant="neutral"
            size="regular"
            onClick={closeDialog}
          >
            {t("common.cancel")}
          </Button>
          <Button
            type="submit"
            form="provider-form"
            variant="solid"
            size="regular"
            disabled={isFormSubmitting || !isFormReady}
          >
            {isFormSubmitting && <Loader2 className="h-4 w-4 animate-spin" />}
            {t("common.save")}
          </Button>
        </>
      }
    >
      {currentNotice}
      {waitingForEditorView ? (
        <div className="py-12 text-center text-body text-fg-2">
          {t("common.loading")}
        </div>
      ) : (
        <ProviderForm
          key={formSessionToken}
          appId={appId}
          providerId={provider.id}
          submitLabel={t("common.save")}
          onSubmit={handleSubmit}
          onCancel={closeDialog}
          onManageAuthAccounts={setAuthSettingsTarget}
          onSubmittingChange={handleSubmittingChange}
          onSubmitReadyChange={handleSubmitReadyChange}
          initialData={initialData}
          showButtons={false}
          isProxyTakeover={isProxyTakeover}
          inactiveFields={editorView?.inactive}
          modeView={modeView}
          onStackLayoutChange={handleStackLayoutChange}
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
