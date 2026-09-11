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
import { extractCodexExperimentalBearerToken } from "@/utils/providerConfigUtils";
import { hermesApi } from "@/lib/api/hermes";

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

const asRecord = (value: unknown): Record<string, unknown> | null =>
  typeof value === "object" && value !== null && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;

const hasAuthMaterial = (value: unknown): boolean => {
  if (value === null || value === undefined) return false;
  if (typeof value === "string") return value.trim().length > 0;
  if (Array.isArray(value)) return value.length > 0;
  if (typeof value === "object") return Object.keys(value).length > 0;
  return true;
};

/**
 * Rebuild the provider auth only for a current Codex provider's live snapshot.
 *
 * In official-auth-preservation mode, live config.toml owns the active
 * provider bearer while the shared auth.json may belong to another provider or
 * contain the user's ChatGPT login. Stored provider auth remains the template:
 * this mirrors the backend switch-away backfill and avoids copying shared auth
 * material into the provider row. DB snapshots and presets must keep their
 * normal auth-first precedence.
 */
const reconcileCodexLiveAuth = (
  liveSettings: Record<string, unknown>,
  storedSettings: Record<string, unknown> | null,
  category: string | undefined,
): Record<string, unknown> => {
  if (category === "official") return liveSettings;

  const configText =
    typeof liveSettings.config === "string" ? liveSettings.config : "";
  const bearer = extractCodexExperimentalBearerToken(configText);
  if (!bearer) return liveSettings;

  const storedAuth = asRecord(storedSettings?.auth);
  const authTemplate = storedAuth ?? asRecord(liveSettings.auth) ?? {};
  const hasProviderApiKey =
    typeof authTemplate.OPENAI_API_KEY === "string" &&
    authTemplate.OPENAI_API_KEY.trim().length > 0;
  const hasOauthLogin = Object.entries(authTemplate).some(
    ([key, value]) =>
      key !== "auth_mode" && key !== "OPENAI_API_KEY" && hasAuthMaterial(value),
  );

  // Match should_restore_codex_provider_token_for_backfill: an OAuth-only
  // provider must not be silently converted into an API-key provider.
  if (hasOauthLogin && !hasProviderApiKey) return liveSettings;

  return {
    ...liveSettings,
    auth: {
      ...authTemplate,
      OPENAI_API_KEY: bearer,
    },
  };
};

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

  // 标记是否已经完成 live 读取（成功、不存在、或跳过读取都视为完成）
  const [hasLoadedLive, setHasLoadedLive] = useState(false);

  // live 读取失败时禁止保存，避免用 DB 快照覆盖实时配置
  const [liveLoadFailed, setLiveLoadFailed] = useState(false);

  // 记录已完成 live 加载的 scope，切换 provider/app 后首个 commit 不会把旧结果当作当前结果
  const [loadedScope, setLoadedScope] = useState<string | null>(null);

  // 失败后的手动重试计数：自增触发加载 effect 重新执行
  const [loadAttempt, setLoadAttempt] = useState(0);

  const currentScope = `${appId}:${provider?.id ?? "new"}`;

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

    // 打开、切换 provider 或切换 app 时，在 effect 同步阶段先重置加载状态
    setLiveSettings(null);
    setHasLoadedLive(false);
    setLiveLoadFailed(false);
    setLoadedScope(null);

    const scope = currentScope;
    const load = async () => {
      if (!open || !provider) {
        return;
      }

      // 代理接管模式：Live 配置已被代理改写，读取 live 会导致编辑界面展示代理地址/占位符等内容
      // 因此直接回退到 SSOT（数据库）配置，避免用户困惑与误保存
      if (isProxyTakeover) {
        if (!cancelled) {
          setHasLoadedLive(true);
          setLoadedScope(scope);
        }
        return;
      }

      // OpenCode uses additive mode, while Pi's shared models.json is owned by
      // the catalog coordinator. Neither has a per-provider generic live
      // snapshot that may replace the DB aggregate in this form, so on the
      // read side the DB snapshot is their sole legitimate source. Claude
      // Desktop's 3P config bundles multiple providers in one file, so the
      // backend read_live_settings() reports "unsupported" by design — a
      // permanent error the failure gate would never let the user past.
      // NOTE this exemption only covers the READ side: when such a
      // provider's node already exists in the live file, saving still
      // overwrites that node with the DB-sourced form values (backend
      // update()'s additive branch). Hermes reads its live fragment through
      // the dedicated branch below; the OpenCode gap needs a single-provider
      // live-read command and is tracked in a #6962 follow-up.
      if (
        appId === "opencode" ||
        appId === "pi" ||
        appId === "claude-desktop"
      ) {
        if (!cancelled) {
          setHasLoadedLive(true);
          setLoadedScope(scope);
        }
        return;
      }

      // Additive apps with a dedicated per-provider live-read command
      // (Option contract: null = no live node yet → DB fallback). The generic
      // path below cannot serve them: ProviderService::current() returns ""
      // for additive apps, so the getCurrent check would never reach a live
      // read. The Hermes fragment already matches the DB's UI shape
      // (snake_case, models array, `_cc_source` marker — write side drops
      // the marker and re-normalizes models back to the YAML dict).
      if (appId === "openclaw" || appId === "hermes") {
        try {
          const live =
            appId === "hermes"
              ? await hermesApi.getLiveProvider(provider.id)
              : await openclawApi.getLiveProvider(provider.id);
          if (!cancelled) {
            if (live && typeof live === "object") {
              setLiveSettings(live);
            }
            setHasLoadedLive(true);
            setLoadedScope(scope);
          }
        } catch {
          if (!cancelled) {
            setLiveLoadFailed(true);
            setHasLoadedLive(true);
            setLoadedScope(scope);
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
            if (!cancelled) {
              if (live && typeof live === "object") {
                setLiveSettings(live);
              }
              setHasLoadedLive(true);
              setLoadedScope(scope);
            }
          } catch {
            if (!cancelled) {
              setLiveLoadFailed(true);
              setHasLoadedLive(true);
              setLoadedScope(scope);
            }
          }
        } else if (!cancelled) {
          setHasLoadedLive(true);
          setLoadedScope(scope);
        }
      } catch {
        // 无法确认当前供应商时也按 live 读取失败处理，避免用 DB 快照覆盖 live
        if (!cancelled) {
          setLiveLoadFailed(true);
          setHasLoadedLive(true);
          setLoadedScope(scope);
        }
      }
    };
    void load();
    return () => {
      cancelled = true;
    };
  }, [open, provider?.id, appId, isProxyTakeover, loadAttempt]); // 只依赖 provider.id，不依赖整个 provider 对象

  const initialSettingsConfig = useMemo(() => {
    const storedSettings = asRecord(provider?.settingsConfig);
    const base =
      appId === "codex" && liveSettings
        ? reconcileCodexLiveAuth(
            liveSettings,
            storedSettings,
            provider?.category,
          )
        : (liveSettings ?? storedSettings ?? {});

    // Codex 的 modelCatalog 是 cc-switch 私有字段，SSOT 在数据库。Live 的 config.toml
    // 仅在写入时投影出 model_catalog_json 指针；Codex.app 改写配置、代理接管/恢复周期、
    // 来回切换供应商都可能让 Live 丢失该投影，从而 read_live_settings 反解为空。
    // 若放任 Live 覆盖，编辑界面会显示空映射表，保存后连同数据库里的映射一起清空（数据丢失）。
    // 因此始终以数据库 SSOT 的 modelCatalog 为准，仅在数据库确实没有时才回退到 Live 反解结果。
    if (
      appId === "codex" &&
      liveSettings &&
      provider?.settingsConfig &&
      typeof provider.settingsConfig === "object"
    ) {
      const dbCatalog = (provider.settingsConfig as Record<string, unknown>)
        .modelCatalog;
      if (dbCatalog !== undefined) {
        return { ...base, modelCatalog: dbCatalog };
      }
    }

    return base;
  }, [liveSettings, provider?.settingsConfig, provider?.category, appId]); // 只依赖表单初始化所需字段，不依赖整个 provider

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

      await onSubmit({
        provider: updatedProvider,
        originalId: provider.id,
      });
      closeDialog();
    },
    [appId, onSubmit, closeDialog, provider],
  );

  const isLiveReady = loadedScope === currentScope && hasLoadedLive;
  const isCurrentFailure = liveLoadFailed && loadedScope === currentScope;

  if (!provider || !initialData) {
    return null;
  }

  if (!isLiveReady || isCurrentFailure) {
    return (
      <FullScreenPanel
        isOpen={open}
        title={t("provider.editProvider")}
        onClose={() => onOpenChange(false)}
        footer={
          <Button
            type="submit"
            form="provider-form"
            disabled
            className="bg-primary text-primary-foreground hover:bg-primary/90"
          >
            <Save className="h-4 w-4 mr-2" />
            {t("common.save")}
          </Button>
        }
      >
        <div
          className={`p-8 text-sm ${
            isCurrentFailure ? "text-destructive" : "text-muted-foreground"
          }`}
          data-testid={isCurrentFailure ? "live-load-error" : undefined}
          role={isCurrentFailure ? "alert" : undefined}
        >
          {isCurrentFailure
            ? t("providerForm.liveLoadFailed", {
                defaultValue: "读取实时配置失败，为避免覆盖实时配置已禁用保存",
              })
            : t("common.loading", { defaultValue: "加载中..." })}
        </div>
        {isCurrentFailure && (
          <div className="px-8 pb-6">
            <Button
              type="button"
              variant="outline"
              data-testid="live-load-retry"
              onClick={() => setLoadAttempt((n) => n + 1)}
            >
              {t("common.retry", { defaultValue: "重试" })}
            </Button>
          </div>
        )}
      </FullScreenPanel>
    );
  }

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
      />
      <AuthSettingsPanel
        target={authSettingsTarget}
        onClose={() => setAuthSettingsTarget(null)}
      />
    </FullScreenPanel>
  );
}
