import { useMemo, useState, useEffect } from "react";
import { AlertTriangle, GripVertical } from "lucide-react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import type {
  DraggableAttributes,
  DraggableSyntheticListeners,
} from "@dnd-kit/core";
import type { OpenClawProviderConfig, Provider } from "@/types";
import type { AppId } from "@/lib/api";
import { authApi } from "@/lib/api";
import { cn } from "@/lib/utils";
import { ProviderActions } from "@/components/providers/ProviderActions";
import { ProviderIcon } from "@/components/ProviderIcon";
import { TEMPLATE_TYPES } from "@/config/constants";
import { isHermesReadOnlyProvider } from "@/config/hermesProviderPresets";
import { ProviderHealthBadge } from "@/components/providers/ProviderHealthBadge";
import { FailoverPriorityBadge } from "@/components/providers/FailoverPriorityBadge";
import { extractCodexExperimentalBearerToken } from "@/utils/providerConfigUtils";
import { resolveManagedAccountId } from "@/lib/authBinding";
import {
  resolveCodexOfficialIdentity,
  supportsOfficialProxyTakeover,
  providerNeedsRouting,
} from "@/utils/providerCapabilities";
import { useProviderHealth } from "@/lib/query/failover";
import { useUsageQuery } from "@/lib/query/queries";
import { resolveProviderIcon } from "@/utils/providerIcon";
import { ProviderStatusBadge } from "@/components/providers/ProviderStatusBadge";
import { isAdditiveAppId, isProxyAppId } from "@/config/appConfig";

interface DragHandleProps {
  attributes: DraggableAttributes;
  listeners: DraggableSyntheticListeners;
  isDragging: boolean;
}

interface ProviderCardProps {
  provider: Provider;
  isCurrent: boolean;
  appId: AppId;
  isInConfig?: boolean; // OpenCode: 是否已添加到 opencode.json
  isOmo?: boolean;
  isOmoSlim?: boolean;
  onSwitch: (provider: Provider) => void;
  onEdit: (provider: Provider) => void;
  onDelete: (provider: Provider) => void;
  onRemoveFromConfig?: (provider: Provider) => void;
  onDisableOmo?: () => void;
  onDisableOmoSlim?: () => void;
  onConfigureUsage: (provider: Provider) => void;
  onOpenWebsite: (url: string) => void;
  onDuplicate: (provider: Provider) => void;
  onTest?: (provider: Provider) => void;
  onOpenTerminal?: (provider: Provider) => void;
  isTesting?: boolean;
  isProxyRunning: boolean;
  isProxyTakeover?: boolean; // 代理接管模式（Live配置已被接管，切换为热切换）
  dragHandleProps?: DragHandleProps;
  /** 点击卡片：在模型列表顶部显示该模型的用量（3 秒后回退当前模型） */
  onShowUsage?: (providerId: string) => void;
  isAutoFailoverEnabled?: boolean; // 是否开启自动故障转移
  failoverPriority?: number; // 故障转移优先级（1 = P1, 2 = P2, ...）
  isInFailoverQueue?: boolean; // 是否在故障转移队列中
  onToggleFailover?: (enabled: boolean) => void; // 切换故障转移队列
  activeProviderId?: string; // 代理当前实际使用的供应商 ID（用于故障转移模式下标注绿色边框）
  // OpenClaw: default model
  isDefaultModel?: boolean;
  isRemovalProtected?: boolean;
  isStateChangeProtected?: boolean;
  onSetAsDefault?: (modelId?: string) => void;
}

/** 判断是否为官方供应商（无自定义 base URL / API key，直连官方 API） */
function isOfficialProvider(provider: Provider, appId: AppId): boolean {
  if (provider.category === "official") {
    return true;
  }

  const config = provider.settingsConfig as Record<string, any>;
  if (appId === "claude") {
    const baseUrl = config?.env?.ANTHROPIC_BASE_URL;
    return !baseUrl || (typeof baseUrl === "string" && baseUrl.trim() === "");
  }
  if (appId === "codex") {
    // 无 OPENAI_API_KEY → 使用 Codex CLI 内置 OAuth（官方）
    const apiKey = config?.auth?.OPENAI_API_KEY;
    const bearerToken =
      typeof config?.config === "string"
        ? extractCodexExperimentalBearerToken(config.config)
        : undefined;
    return (
      !bearerToken &&
      (!apiKey || (typeof apiKey === "string" && apiKey.trim() === ""))
    );
  }
  if (appId === "gemini") {
    // 无 GEMINI_API_KEY 且无 GOOGLE_GEMINI_BASE_URL → Google OAuth 官方模式
    const apiKey = config?.env?.GEMINI_API_KEY;
    const baseUrl = config?.env?.GOOGLE_GEMINI_BASE_URL;
    return (
      (!apiKey || (typeof apiKey === "string" && apiKey.trim() === "")) &&
      (!baseUrl || (typeof baseUrl === "string" && baseUrl.trim() === ""))
    );
  }
  return false;
}

export function ProviderCard({
  provider,
  isCurrent,
  appId,
  isInConfig = true,
  isOmo = false,
  isOmoSlim = false,
  onSwitch,
  onEdit,
  onDelete,
  onRemoveFromConfig,
  onDisableOmo,
  onDisableOmoSlim,
  onConfigureUsage,
  onDuplicate,
  onTest,
  onOpenTerminal,
  onShowUsage,
  isTesting,
  isProxyRunning,
  isProxyTakeover = false,
  dragHandleProps,
  isAutoFailoverEnabled = false,
  failoverPriority,
  isInFailoverQueue = false,
  onToggleFailover,
  activeProviderId,
  // OpenClaw: default model
  isDefaultModel,
  isRemovalProtected,
  isStateChangeProtected,
  onSetAsDefault,
}: ProviderCardProps) {
  const { t } = useTranslation();
  const codexOfficialIdentity = resolveCodexOfficialIdentity(appId, provider);
  const managedCodexAccountId = resolveManagedAccountId(
    provider.meta,
    "codex_oauth",
  )?.trim();
  const {
    data: codexAuthStatus,
    isSuccess: isCodexAuthStatusSuccess,
    isError: isCodexAuthStatusError,
  } = useQuery({
    queryKey: ["managed-auth-status", "codex_oauth"],
    queryFn: () => authApi.authGetStatus("codex_oauth"),
    enabled:
      codexOfficialIdentity === "managed_account" &&
      Boolean(managedCodexAccountId),
    staleTime: 30_000,
  });
  const managedCodexAccount = codexAuthStatus?.accounts.find(
    (account) => account.id === managedCodexAccountId,
  );
  const manualNote = provider.notes?.trim() || undefined;
  const providerNameIncludesAccountLogin = Boolean(
    managedCodexAccount?.login &&
      (provider.name.trim() === managedCodexAccount.login ||
        provider.name.trim() ===
          `OpenAI Official (${managedCodexAccount.login})`),
  );

  // OMO and OMO Slim share the same card behavior
  const isAnyOmo = isOmo || isOmoSlim;
  const handleDisableAnyOmo = isOmoSlim ? onDisableOmoSlim : onDisableOmo;
  const isAdditiveMode =
    (appId === "opencode" && !isAnyOmo) || appId === "pi" || appId === "mcode";

  const { data: health } = useProviderHealth(
    provider.id,
    appId,
    isProxyAppId(appId),
  );

  const openclawDefaultModelOptions = useMemo(() => {
    if (appId !== "openclaw") return [];
    const config = provider.settingsConfig as OpenClawProviderConfig;
    if (!Array.isArray(config?.models)) return [];
    return config.models
      .filter((model) => typeof model.id === "string" && model.id.trim())
      .map((model) => ({ id: model.id, name: model.name }));
  }, [appId, provider.settingsConfig]);

  const isBoundCodexOfficial = codexOfficialIdentity === "managed_account";
  const usageEnabled =
    provider.meta?.usage_script?.enabled ?? isBoundCodexOfficial;
  const isOfficial = isOfficialProvider(provider, appId);
  const supportsOfficialSubscription =
    isOfficial && ["claude", "codex", "gemini", "grokbuild"].includes(appId);
  const isOfficialSubscriptionUsage =
    provider.meta?.usage_script?.templateType ===
    TEMPLATE_TYPES.OFFICIAL_SUBSCRIPTION;
  // 官方判定只认显式 category === "official"（SSOT），不回退 isOfficial 的空字段启发式。
  // 理由（此判定曾在「纯 category ↔ category+isOfficial 回退」间反复，结论钉死于此）：
  //  1) 封号保护是高代价决策，不该建立在「base_url/key 缺失」这种脆弱信号上——它无法区分
  //     「想直连官方」与「自定义但还没填完」，两者都表现为字段为空，必然误伤后者。
  //  2) 启发式在 UI 多拦的部分，执行层 useProviderActions.ts 也只认 category === "official"、
  //     并不兑现（绕过 UI 即可切换）→ 属虚保护，却以误伤 category 缺失的自定义供应商为代价。
  //  3) 预设导入的官方一定带 category="official"，category 缺失的「真官方」现实中≈不存在。
  // 真官方就该有显式 category；手动新建官方应引导标注，而不是靠空字段猜。
  const supportsOfficialRouting = supportsOfficialProxyTakeover(
    appId,
    provider,
  );
  const isOfficialBlockedByProxy =
    isProxyTakeover &&
    provider.category === "official" &&
    !supportsOfficialRouting;
  // Hermes v12+ overlay entries live under the `providers:` dict and are
  // read-only here — writes have to go through Hermes Web UI.
  const isHermesReadOnly =
    appId === "hermes" && isHermesReadOnlyProvider(provider.settingsConfig);
  // 统一权威谓词（详见 providerNeedsRouting）：以 providerType 为准，不受
  // apiFormat 被改动/缺省影响。此 badge 仅在 Codex 视图渲染，故加 appId 守卫。
  const codexNeedsRouting =
    appId === "codex" && providerNeedsRouting(appId, provider);
  // 用量查询仅服务于右键菜单「刷新用量」（卡片本身不展示用量）
  const shouldAutoQuery = isAdditiveAppId(appId) ? isInConfig : isCurrent;
  const autoQueryInterval = shouldAutoQuery
    ? provider.meta?.usage_script?.autoQueryInterval || 0
    : 0;

  const { refetch: refetchUsage } = useUsageQuery(provider.id, appId, {
    enabled: usageEnabled && !isOfficial && !isOfficialSubscriptionUsage,
    autoQueryInterval,
  });

  // 判断是否是"当前使用中"的供应商
  // - OMO/OMO Slim 供应商：使用 isCurrent
  // - OpenClaw：使用默认模型归属的 provider 作为当前项（蓝色边框）
  // - OpenCode（非 OMO）：不存在"当前"概念，返回 false
  // - 故障转移模式：代理实际使用的供应商（activeProviderId）
  // - 普通模式：isCurrent
  const isActiveProvider = isAnyOmo
    ? isCurrent
    : appId === "openclaw"
      ? Boolean(isDefaultModel)
      : appId === "opencode" || appId === "pi" || appId === "mcode"
        ? false
        : isAutoFailoverEnabled
          ? activeProviderId === provider.id
          : isCurrent;

  const shouldUseGreen = !isAnyOmo && isProxyTakeover && isActiveProvider;
  const hasPersistentConfigHighlight = isAdditiveMode && isInConfig;
  const shouldUseBlue =
    (isAnyOmo && isActiveProvider) ||
    (!isAnyOmo &&
      !isProxyTakeover &&
      (isActiveProvider || hasPersistentConfigHighlight));
  const hasStateHighlight = shouldUseGreen || shouldUseBlue;

  // 右键菜单：在鼠标位置弹出操作菜单（自绘，避免依赖全局容器）
  const [ctxMenu, setCtxMenu] = useState<{ x: number; y: number } | null>(null);
  const handleContextMenu = (event: React.MouseEvent) => {
    event.preventDefault();
    const menuWidth = 200;
    const menuHeight = 320;
    setCtxMenu({
      x: Math.min(event.clientX, window.innerWidth - menuWidth - 8),
      y: Math.min(event.clientY, window.innerHeight - menuHeight - 8),
    });
  };
  useEffect(() => {
    if (!ctxMenu) return;
    const close = () => setCtxMenu(null);
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") setCtxMenu(null);
    };
    window.addEventListener("mousedown", close);
    window.addEventListener("keydown", onKey);
    window.addEventListener("blur", close);
    return () => {
      window.removeEventListener("mousedown", close);
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("blur", close);
    };
  }, [ctxMenu]);

  // 主点击 = 切换供应商（与右键菜单主动作一致）。
  // OMO 当前项 / 累加模式（添加即切换的成员关系）保留「固定用量条」，
  // 避免误点触发移除或停用；故障转移模式下主点击 = 加入/移除队列。
  const handleCardClick = () => {
    if (isAnyOmo) {
      if (!isCurrent) {
        onSwitch(provider);
      } else {
        onShowUsage?.(provider.id);
      }
      return;
    }
    if (isAdditiveMode) {
      onShowUsage?.(provider.id);
      return;
    }
    if (isAutoFailoverEnabled && onToggleFailover) {
      onToggleFailover(!isInFailoverQueue);
      return;
    }
    if (!isCurrent) {
      onSwitch(provider);
    } else {
      onShowUsage?.(provider.id);
    }
  };

  return (
    <div
      onContextMenu={handleContextMenu}
      onClick={handleCardClick}
      className={cn(
        "relative overflow-hidden rounded-xl border border-border p-2.5 transition-all duration-300",
        "bg-card text-card-foreground group cursor-pointer",
        isAutoFailoverEnabled || isProxyTakeover
          ? "hover:border-emerald-500/50"
          : "hover:border-border-active",
        shouldUseGreen &&
          "border-emerald-500/60 shadow-sm shadow-emerald-500/10",
        shouldUseBlue && "border-blue-500/60 shadow-sm shadow-blue-500/10",
        !hasStateHighlight && "hover:shadow-sm",
        dragHandleProps?.isDragging &&
          "cursor-grabbing border-primary shadow-lg scale-105 z-10",
      )}
    >
      <div
        className={cn(
          "absolute inset-0 bg-gradient-to-r to-transparent transition-opacity duration-500 pointer-events-none",
          shouldUseGreen && "from-emerald-500/10",
          shouldUseBlue && "from-blue-500/10",
          !hasStateHighlight && "from-primary/10",
          hasStateHighlight ? "opacity-100" : "opacity-0",
        )}
      />
      <div className="relative flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
        <div className="flex min-w-0 flex-1 items-center gap-1.5">
          {dragHandleProps && (
            <button
              type="button"
              className={cn(
                "-ml-1 flex-shrink-0 cursor-grab active:cursor-grabbing p-1",
                "text-muted-foreground/50 hover:text-muted-foreground transition-colors",
                dragHandleProps.isDragging && "cursor-grabbing",
              )}
              aria-label={t("provider.dragHandle")}
              {...dragHandleProps.attributes}
              {...dragHandleProps.listeners}
            >
              <GripVertical className="h-3.5 w-3.5" />
            </button>
          )}

          <div className="h-7 w-7 flex-shrink-0 rounded-lg bg-muted flex items-center justify-center border border-border group-hover:scale-105 transition-transform duration-300">
            <ProviderIcon
              icon={resolveProviderIcon(
                appId,
                provider.icon,
                provider.iconColor,
              )}
              name={provider.name}
              color={provider.iconColor}
              size={16}
            />
          </div>

          <div className="min-w-0 flex-1 space-y-0.5">
            <div className="flex flex-wrap items-center gap-1.5 min-h-6">
              <h3
                className="min-w-0 flex-1 truncate text-sm font-semibold leading-none"
                title={provider.name}
              >
                {provider.name}
              </h3>

              {isOmo && (
                <span className="inline-flex items-center rounded-md bg-violet-100 px-1.5 py-0.5 text-[10px] font-semibold text-violet-700 dark:bg-violet-900/40 dark:text-violet-300">
                  OMO
                </span>
              )}

              {isOmoSlim && (
                <span className="inline-flex items-center rounded-md bg-indigo-100 px-1.5 py-0.5 text-[10px] font-semibold text-indigo-700 dark:bg-indigo-900/40 dark:text-indigo-300">
                  Slim
                </span>
              )}

              {appId === "claude-desktop" &&
                providerNeedsRouting(appId, provider) && (
                  <ProviderStatusBadge
                    tone="info"
                    label={t("provider.needsRouting", {
                      defaultValue: "需要路由",
                    })}
                  />
                )}

              {appId === "claude" && providerNeedsRouting(appId, provider) && (
                <ProviderStatusBadge
                  tone="info"
                  label={t("provider.needsRouting", {
                    defaultValue: "需要路由",
                  })}
                />
              )}

              {codexNeedsRouting && (
                <ProviderStatusBadge
                  tone="info"
                  label={t("provider.needsRouting", {
                    defaultValue: "需要路由",
                  })}
                />
              )}

              {appId === "claude" && provider.category === "official" && (
                <ProviderStatusBadge
                  label={t("provider.noRoutingSupport", {
                    defaultValue: "不支持路由",
                  })}
                />
              )}

              {isProxyRunning &&
                !supportsOfficialRouting &&
                isInFailoverQueue &&
                health && (
                  <ProviderHealthBadge
                    consecutiveFailures={health.consecutive_failures}
                    isHealthy={health.is_healthy}
                  />
                )}

              {isAutoFailoverEnabled &&
                !supportsOfficialRouting &&
                isInFailoverQueue &&
                failoverPriority && (
                  <FailoverPriorityBadge priority={failoverPriority} />
                )}

              {isHermesReadOnly && (
                <span
                  className="inline-flex items-center rounded-md bg-slate-200 px-1.5 py-0.5 text-[10px] font-semibold text-slate-700 dark:bg-slate-700/60 dark:text-slate-200"
                  title={t("provider.managedByHermesHint", {
                    defaultValue: "由 Hermes 管理，请在 Hermes Web UI 中编辑",
                  })}
                >
                  {t("provider.managedByHermes", {
                    defaultValue: "Hermes Managed",
                  })}
                </span>
              )}
            </div>

            {manualNote?.trim() ? (
              <p
                className="min-w-0 truncate text-[11px] text-muted-foreground"
                title={manualNote.trim()}
              >
                {manualNote.trim()}
              </p>
            ) : codexOfficialIdentity && codexOfficialIdentity !== "api_key" ? (
              <div className="flex min-w-0 items-center gap-2 text-sm text-muted-foreground">
                {codexOfficialIdentity === "native_login" ? (
                  <span className="min-w-0 truncate" title={manualNote}>
                    {manualNote ??
                      t("codex.followCodexLoginDescription", {
                        defaultValue: "账号会随 Codex CLI 当前登录变化",
                      })}
                  </span>
                ) : managedCodexAccount ? (
                  <>
                    <span
                      className="min-w-0 truncate"
                      title={manualNote ?? managedCodexAccount.login}
                    >
                      {manualNote ??
                        (providerNameIncludesAccountLogin
                          ? t("codex.openAiAccount", {
                              defaultValue: "OpenAI 账号",
                            })
                          : managedCodexAccount.login)}
                    </span>
                    {managedCodexAccount.reauth_required && (
                      <span className="inline-flex shrink-0 items-center gap-1 text-amber-700 dark:text-amber-300">
                        <AlertTriangle className="h-3.5 w-3.5" />
                        {t("codexOauth.reauthBadge", "需要重新登录")}
                      </span>
                    )}
                  </>
                ) : isCodexAuthStatusError ? (
                  <span className="inline-flex min-w-0 items-center gap-1 text-amber-700 dark:text-amber-300">
                    <AlertTriangle className="h-3.5 w-3.5 shrink-0" />
                    <span className="truncate">
                      {t("codex.accountStatusUnavailable", {
                        defaultValue: "无法读取账号信息",
                      })}
                    </span>
                  </span>
                ) : isCodexAuthStatusSuccess ? (
                  <>
                    <span className="inline-flex min-w-0 items-center gap-1 text-sm text-amber-700 dark:text-amber-300">
                      <AlertTriangle className="h-3.5 w-3.5 shrink-0" />
                      <span className="truncate">
                        {t("codex.boundAccountUnavailable", {
                          defaultValue: "绑定的账号不可用",
                        })}
                      </span>
                    </span>
                    <button
                      type="button"
                      className="shrink-0 text-sm font-medium text-primary hover:underline"
                      onClick={() => onEdit(provider)}
                    >
                      {t("codex.chooseAccount", {
                        defaultValue: "选择账号",
                      })}
                    </button>
                  </>
                ) : (
                  <span className="min-w-0 truncate">
                    {t("codex.accountLoading", {
                      defaultValue: "正在加载账号…",
                    })}
                  </span>
                )}
              </div>
            ) : null}
          </div>
        </div>
      </div>

      {/* 右键操作菜单（替代 hover 按钮组：启动/编辑/复制/检测/用量/终端/删除） */}
      {ctxMenu && (
        <div
          className="fixed z-[100] rounded-lg border border-zinc-800 bg-[#161b22]/95 shadow-2xl shadow-black/40 backdrop-blur"
          style={{ left: ctxMenu.x, top: ctxMenu.y }}
          onMouseDown={(event) => event.stopPropagation()}
          // 菜单是卡片 DOM 的子元素：不拦截 click 会冒泡到卡片主点击，
          // 点「删除」等菜单项时会顺带把该供应商切换为当前项（进而
          // 触发"当前供应商不可删除"），表现为"删除不了"。
          onClick={(event) => event.stopPropagation()}
          onContextMenu={(event) => {
            event.preventDefault();
            event.stopPropagation();
          }}
        >
          <ProviderActions
            appId={appId}
            isCurrent={isCurrent}
            isInConfig={isInConfig}
            isTesting={isTesting}
            isProxyTakeover={isProxyTakeover}
            isOfficialBlockedByProxy={isOfficialBlockedByProxy}
            isReadOnly={isHermesReadOnly}
            isOmo={isAnyOmo}
            onSwitch={() => onSwitch(provider)}
            onEdit={() => onEdit(provider)}
            onDuplicate={() => onDuplicate(provider)}
            onTest={
              // 连通检测对第三方/自定义/Copilot/Codex-OAuth 供应商开放（这些正是旧的
              // 真实请求探测会误报、而可达性探测能正确处理的对象）。官方供应商
              // (category === "official") 一律隐藏：它们 base_url 故意留空、走客户端
              // 默认/OAuth 端点，cc-switch 没有可靠的探测目标（尤其 Claude Desktop
              // 官方是原生 1P 模式，根本不在请求路径上）。
              onTest && appId !== "mcode" && provider.category !== "official"
                ? () => onTest(provider)
                : undefined
            }
            onConfigureUsage={
              isOfficial && !supportsOfficialSubscription
                ? undefined
                : () => onConfigureUsage(provider)
            }
            onRefreshUsage={
              usageEnabled && !isOfficial
                ? () => void refetchUsage()
                : undefined
            }
            onDelete={() => onDelete(provider)}
            onRemoveFromConfig={
              onRemoveFromConfig
                ? () => onRemoveFromConfig(provider)
                : undefined
            }
            onDisableOmo={handleDisableAnyOmo}
            onOpenTerminal={
              onOpenTerminal ? () => onOpenTerminal(provider) : undefined
            }
            isAutoFailoverEnabled={isAutoFailoverEnabled}
            isInFailoverQueue={isInFailoverQueue}
            onToggleFailover={
              supportsOfficialRouting ? undefined : onToggleFailover
            }
            // OpenClaw: default model
            isDefaultModel={isDefaultModel}
            isRemovalProtected={isRemovalProtected}
            isStateChangeProtected={isStateChangeProtected}
            defaultModelOptions={openclawDefaultModelOptions}
            onSetAsDefault={onSetAsDefault}
          />
        </div>
      )}
    </div>
  );
}
