import { useMemo, useState, useEffect } from "react";
import { GripVertical, ChevronDown, ChevronUp } from "lucide-react";
import { useTranslation } from "react-i18next";
import { motion } from "framer-motion";
import type {
  DraggableAttributes,
  DraggableSyntheticListeners,
} from "@dnd-kit/core";
import type { Provider } from "@/types";
import type { AppId } from "@/lib/api";
import { cn } from "@/lib/utils";
import { ProviderActions } from "@/components/providers/ProviderActions";
import { ProviderIcon } from "@/components/ProviderIcon";
import UsageFooter from "@/components/UsageFooter";
import SubscriptionQuotaFooter from "@/components/SubscriptionQuotaFooter";
import CopilotQuotaFooter from "@/components/CopilotQuotaFooter";
import CodexOauthQuotaFooter from "@/components/CodexOauthQuotaFooter";
import { PROVIDER_TYPES } from "@/config/constants";
import { isHermesReadOnlyProvider } from "@/config/hermesProviderPresets";
import { ProviderHealthBadge } from "@/components/providers/ProviderHealthBadge";
import { FailoverPriorityBadge } from "@/components/providers/FailoverPriorityBadge";
import { extractCodexBaseUrl } from "@/utils/providerConfigUtils";
import { useProviderHealth } from "@/lib/query/failover";
import { useUsageQuery } from "@/lib/query/queries";

interface DragHandleProps {
  attributes: DraggableAttributes;
  listeners: DraggableSyntheticListeners;
  isDragging: boolean;
}

interface ProviderCardProps {
  provider: Provider;
  isCurrent: boolean;
  appId: AppId;
  isInConfig?: boolean;
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
  isProxyTakeover?: boolean;
  dragHandleProps?: DragHandleProps;
  isAutoFailoverEnabled?: boolean;
  failoverPriority?: number;
  isInFailoverQueue?: boolean;
  onToggleFailover?: (enabled: boolean) => void;
  activeProviderId?: string;
  isDefaultModel?: boolean;
  onSetAsDefault?: () => void;
  staggerIndex?: number;
}

function isOfficialProvider(provider: Provider, appId: AppId): boolean {
  const config = provider.settingsConfig as Record<string, any>;
  if (appId === "claude") {
    const baseUrl = config?.env?.ANTHROPIC_BASE_URL;
    return !baseUrl || (typeof baseUrl === "string" && baseUrl.trim() === "");
  }
  if (appId === "codex") {
    const apiKey = config?.auth?.OPENAI_API_KEY;
    return !apiKey || (typeof apiKey === "string" && apiKey.trim() === "");
  }
  if (appId === "gemini") {
    const apiKey = config?.env?.GEMINI_API_KEY;
    const baseUrl = config?.env?.GOOGLE_GEMINI_BASE_URL;
    return (
      (!apiKey || (typeof apiKey === "string" && apiKey.trim() === "")) &&
      (!baseUrl || (typeof baseUrl === "string" && baseUrl.trim() === ""))
    );
  }
  return false;
}

const extractApiUrl = (provider: Provider, fallbackText: string) => {
  if (provider.notes?.trim()) return provider.notes.trim();
  if (provider.websiteUrl) return provider.websiteUrl;

  const config = provider.settingsConfig;
  if (config && typeof config === "object") {
    const envBase =
      (config as Record<string, any>)?.env?.ANTHROPIC_BASE_URL ||
      (config as Record<string, any>)?.env?.GOOGLE_GEMINI_BASE_URL;
    if (typeof envBase === "string" && envBase.trim()) return envBase;

    const baseUrl = (config as Record<string, any>)?.config;
    if (typeof baseUrl === "string" && baseUrl.includes("base_url")) {
      const extracted = extractCodexBaseUrl(baseUrl);
      if (extracted) return extracted;
    }
  }
  return fallbackText;
};

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
  onOpenWebsite,
  onDuplicate,
  onTest,
  onOpenTerminal,
  isTesting,
  isProxyRunning,
  isProxyTakeover = false,
  dragHandleProps,
  isAutoFailoverEnabled = false,
  failoverPriority,
  isInFailoverQueue = false,
  onToggleFailover,
  activeProviderId,
  isDefaultModel,
  onSetAsDefault,
  staggerIndex,
}: ProviderCardProps) {
  const { t } = useTranslation();

  const isAnyOmo = isOmo || isOmoSlim;
  const handleDisableAnyOmo = isOmoSlim ? onDisableOmoSlim : onDisableOmo;
  const isAdditiveMode = appId === "opencode" && !isAnyOmo;

  const { data: health } = useProviderHealth(provider.id, appId);

  const fallbackUrlText = t("provider.notConfigured", {
    defaultValue: "未配置接口地址",
  });

  const displayUrl = useMemo(
    () => extractApiUrl(provider, fallbackUrlText),
    [provider, fallbackUrlText],
  );

  const isClickableUrl = useMemo(() => {
    if (provider.notes?.trim()) return false;
    if (displayUrl === fallbackUrlText) return false;
    return true;
  }, [provider.notes, displayUrl, fallbackUrlText]);

  const usageEnabled = provider.meta?.usage_script?.enabled ?? false;
  const isOfficial = isOfficialProvider(provider, appId);
  const isOfficialBlockedByProxy =
    isProxyTakeover && (provider.category === "official" || isOfficial);
  const isCopilot =
    provider.meta?.providerType === PROVIDER_TYPES.GITHUB_COPILOT ||
    provider.meta?.usage_script?.templateType === "github_copilot";
  const isHermesReadOnly =
    appId === "hermes" && isHermesReadOnlyProvider(provider.settingsConfig);
  const isCodexOauth =
    provider.meta?.providerType === PROVIDER_TYPES.CODEX_OAUTH;

  const shouldAutoQuery =
    appId === "opencode" || appId === "openclaw" || appId === "hermes"
      ? isInConfig
      : isCurrent;
  const autoQueryInterval = shouldAutoQuery
    ? provider.meta?.usage_script?.autoQueryInterval || 0
    : 0;

  const { data: usage } = useUsageQuery(provider.id, appId, {
    enabled: usageEnabled,
    autoQueryInterval,
  });

  const isTokenPlan =
    provider.meta?.usage_script?.templateType === "token_plan";
  const hasMultiplePlans =
    usage?.success && usage.data && usage.data.length > 1 && !isTokenPlan;

  const [isExpanded, setIsExpanded] = useState(false);

  useEffect(() => {
    if (hasMultiplePlans) setIsExpanded(true);
  }, [hasMultiplePlans]);

  const handleOpenWebsite = () => {
    if (!isClickableUrl) return;
    onOpenWebsite(displayUrl);
  };

  const isActiveProvider = isAnyOmo
    ? isCurrent
    : appId === "openclaw"
      ? Boolean(isDefaultModel)
      : appId === "opencode"
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

  return (
    <motion.div
      layout
      initial={{ opacity: 0, y: 8, scale: 0.98 }}
      animate={{ opacity: 1, y: 0, scale: 1 }}
      exit={{ opacity: 0, transition: { duration: 0.08 } }}
      transition={{
        type: "spring",
        stiffness: 500,
        damping: 32,
        mass: 0.6,
        delay: staggerIndex ? Math.min(staggerIndex * 0.03, 0.2) : 0,
      }}
      whileHover={{ y: -2, transition: { type: "spring", stiffness: 600, damping: 28 } }}
      whileTap={{ scale: 0.98, transition: { type: "spring", stiffness: 700, damping: 30 } }}
      className={cn(
        "group relative overflow-hidden rounded-2xl p-4 cursor-default",
        /* Default: liquid glass */
        !shouldUseGreen && !shouldUseBlue && [
          "liquid-glass",
          "hover:bg-white/50 dark:hover:bg-white/[0.06]",
        ],
        /* Active: emerald (proxy takeover) */
        shouldUseGreen && "liquid-glass-emerald",
        /* Active: primary (current provider) */
        shouldUseBlue && "liquid-glass-active",
        /* Dragging */
        dragHandleProps?.isDragging && "shadow-xl z-10",
      )}
      style={dragHandleProps?.isDragging ? { scale: 1.03, rotate: 0.5, zIndex: 10 } : undefined}
    >
      <div className="relative flex flex-col gap-3.5 sm:flex-row sm:items-center sm:justify-between">
        <div className="flex flex-1 items-center gap-2.5 min-w-0">
          <button
            type="button"
            className={cn(
              "-ml-1 flex-shrink-0 cursor-grab active:cursor-grabbing p-1.5 rounded-lg",
              "text-muted-foreground/30 hover:text-muted-foreground/60 hover:bg-white/30 dark:hover:bg-white/5 transition-all",
              dragHandleProps?.isDragging && "cursor-grabbing",
            )}
            aria-label={t("provider.dragHandle")}
            {...(dragHandleProps?.attributes ?? {})}
            {...(dragHandleProps?.listeners ?? {})}
          >
            <GripVertical className="h-3.5 w-3.5" />
          </button>

          {/* Provider icon — glass bubble */}
          <div
            className={cn(
              "relative h-9 w-9 rounded-xl flex items-center justify-center flex-shrink-0",
              "liquid-glass-subtle",
              "group-hover:bg-white/40 dark:group-hover:bg-white/[0.06]",
              "transition-all duration-200",
            )}
          >
            <ProviderIcon
              icon={provider.icon}
              name={provider.name}
              color={provider.iconColor}
              size={20}
            />
            {/* Subtle active indicator dot */}
            {(isActiveProvider || hasPersistentConfigHighlight) && (
              <div
                className={cn(
                  "absolute -top-0.5 -right-0.5 h-2 w-2 rounded-full ring-2 ring-card",
                  shouldUseGreen ? "bg-emerald-500" : "bg-primary",
                )}
              />
            )}
          </div>

          <div className="space-y-0.5 min-w-0">
            <div className="flex flex-wrap items-center gap-1.5 min-h-[1.375rem]">
              <h3 className="text-sm font-semibold leading-none tracking-tight truncate">
                {provider.name}
              </h3>

              {isOmo && (
                <span className="inline-flex items-center rounded bg-violet-500/10 px-1.5 py-px text-2xs font-semibold text-violet-600 dark:text-violet-400 ring-1 ring-violet-500/20">
                  OMO
                </span>
              )}

              {isOmoSlim && (
                <span className="inline-flex items-center rounded bg-indigo-500/10 px-1.5 py-px text-2xs font-semibold text-indigo-600 dark:text-indigo-400 ring-1 ring-indigo-500/20">
                  Slim
                </span>
              )}

              {isProxyRunning && isInFailoverQueue && health && (
                <ProviderHealthBadge
                  consecutiveFailures={health.consecutive_failures}
                />
              )}

              {isAutoFailoverEnabled &&
                isInFailoverQueue &&
                failoverPriority && (
                  <FailoverPriorityBadge priority={failoverPriority} />
                )}

              {provider.category === "third_party" &&
                provider.meta?.isPartner && (
                  <span
                    className="text-amber-500 dark:text-amber-400 text-xs"
                    title={t("provider.officialPartner", {
                      defaultValue: "官方合作伙伴",
                    })}
                  >
                    ★
                  </span>
                )}

              {isHermesReadOnly && (
                <span className="inline-flex items-center rounded bg-slate-500/10 px-1.5 py-px text-2xs font-medium text-slate-600 dark:text-slate-400 ring-1 ring-slate-500/20">
                  {t("provider.managedByHermes", {
                    defaultValue: "Hermes Managed",
                  })}
                </span>
              )}
            </div>

            {displayUrl && (
              <button
                type="button"
                onClick={handleOpenWebsite}
                className={cn(
                  "inline-flex items-center text-xs max-w-[260px] truncate",
                  isClickableUrl
                    ? "text-primary/70 hover:text-primary hover:underline cursor-pointer transition-colors"
                    : "text-muted-foreground/60 cursor-default",
                )}
                title={displayUrl}
                disabled={!isClickableUrl}
              >
                {displayUrl}
              </button>
            )}
          </div>
        </div>

        <div className="flex items-center ml-auto min-w-0 gap-2.5">
          <div className="ml-auto">
            <div className="flex items-center gap-1.5">
              {isCopilot ? (
                <CopilotQuotaFooter
                  meta={provider.meta}
                  inline={true}
                  isCurrent={isCurrent}
                />
              ) : isCodexOauth ? (
                <CodexOauthQuotaFooter
                  meta={provider.meta}
                  inline={true}
                  isCurrent={isCurrent}
                />
              ) : isOfficial ? (
                <SubscriptionQuotaFooter
                  appId={appId}
                  inline={true}
                  isCurrent={isCurrent}
                />
              ) : hasMultiplePlans ? (
                <span className="text-xs font-medium text-muted-foreground">
                  {t("usage.multiplePlans", {
                    count: usage?.data?.length || 0,
                    defaultValue: `${usage?.data?.length || 0} 个套餐`,
                  })}
                </span>
              ) : (
                <UsageFooter
                  provider={provider}
                  providerId={provider.id}
                  appId={appId}
                  usageEnabled={usageEnabled}
                  isCurrent={isCurrent}
                  isInConfig={isInConfig}
                  inline={true}
                />
              )}
              {hasMultiplePlans && (
                <button
                  onClick={(e) => {
                    e.stopPropagation();
                    setIsExpanded(!isExpanded);
                  }}
                  className="p-1 rounded-md hover:bg-muted/80 transition-colors text-muted-foreground/60 hover:text-muted-foreground flex-shrink-0"
                  title={
                    isExpanded
                      ? t("usage.collapse", { defaultValue: "收起" })
                      : t("usage.expand", { defaultValue: "展开" })
                  }
                >
                  {isExpanded ? (
                    <ChevronUp size={13} />
                  ) : (
                    <ChevronDown size={13} />
                  )}
                </button>
              )}
            </div>
          </div>

          {/* Action buttons — appear on hover */}
          <div
            className={cn(
              "flex items-center gap-1 flex-shrink-0",
              "opacity-0 pointer-events-none",
              "group-hover:opacity-100 group-hover:pointer-events-auto",
              "group-focus-within:opacity-100 group-focus-within:pointer-events-auto",
              "transition-opacity duration-150",
            )}
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
                onTest && !isOfficial && !isCopilot && !isCodexOauth
                  ? () => onTest(provider)
                  : undefined
              }
              onConfigureUsage={
                isOfficial || isCopilot || isCodexOauth
                  ? undefined
                  : () => onConfigureUsage(provider)
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
              onToggleFailover={onToggleFailover}
              isDefaultModel={isDefaultModel}
              onSetAsDefault={onSetAsDefault}
            />
          </div>
        </div>
      </div>

      {isExpanded && hasMultiplePlans && (
        <div className="mt-3.5 pt-3.5 border-t border-white/10 dark:border-white/5">
          <UsageFooter
            provider={provider}
            providerId={provider.id}
            appId={appId}
            usageEnabled={usageEnabled}
            isCurrent={isCurrent}
            isInConfig={isInConfig}
            inline={false}
          />
        </div>
      )}
    </motion.div>
  );
}
