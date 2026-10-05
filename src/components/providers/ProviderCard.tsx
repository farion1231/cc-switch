import { useMemo, useState, useEffect } from "react";
import {
  AlertTriangle,
  GripVertical,
  ChevronDown,
  ChevronUp,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import type {
  DraggableAttributes,
  DraggableSyntheticListeners,
} from "@dnd-kit/core";
import type { Provider } from "@/types";
import type { AppId } from "@/lib/api";
import { authApi } from "@/lib/api";
import { cn } from "@/lib/utils";
import { ProviderIcon } from "@/components/ProviderIcon";
import { HoverTip } from "@/components/ui/hover-tip";
import UsageFooter from "@/components/UsageFooter";
import SubscriptionQuotaFooter from "@/components/SubscriptionQuotaFooter";
import CopilotQuotaFooter from "@/components/CopilotQuotaFooter";
import CodexOauthQuotaFooter from "@/components/CodexOauthQuotaFooter";
import XaiOauthQuotaFooter from "@/components/XaiOauthQuotaFooter";
import { PROVIDER_TYPES, TEMPLATE_TYPES } from "@/config/constants";
import {
  extractCodexBaseUrl,
  extractCodexExperimentalBearerToken,
} from "@/utils/providerConfigUtils";
import { resolveManagedAccountId } from "@/lib/authBinding";
import { resolveCodexOfficialIdentity } from "@/utils/providerCapabilities";
import { useProviderHealth } from "@/lib/query/failover";
import { useUsageQuery } from "@/lib/query/queries";
import { resolveProviderIcon } from "@/utils/providerIcon";
import { isAdditiveAppId, isProxyAppId } from "@/config/appConfig";
import { ProviderCardActions } from "./ProviderCardActions";
import type { CardChip, CardPresentation, CardTone } from "./presentation";

interface DragHandleProps {
  attributes: DraggableAttributes;
  listeners: DraggableSyntheticListeners;
  isDragging: boolean;
}

interface ProviderCardProps {
  provider: Provider;
  appId: AppId;
  /** 卡片怎么画：模式色、状态 / 按钮、徽标（由列表按模式算好） */
  presentation: CardPresentation;
  /** 用来决定额度是否自动查询（共存式应用看 isInConfig） */
  isCurrent: boolean;
  isInConfig?: boolean;
  onEdit: (provider: Provider) => void;
  onDelete: (provider: Provider) => void;
  onConfigureUsage: (provider: Provider) => void;
  onOpenWebsite: (url: string) => void;
  onDuplicate?: (provider: Provider) => void;
  onTest?: (provider: Provider) => void;
  onOpenTerminal?: (provider: Provider) => void;
  isTesting?: boolean;
  dragHandleProps?: DragHandleProps;
}

/** 当前那张：模式色边框 + 淡底；共存式「已添加」是中性的。 */
const TONE_CLASS: Record<CardTone, string> = {
  direct: "border-direct-border bg-direct-soft",
  route: "border-route-border bg-route-soft",
  stack: "border-stack-border bg-stack-soft",
  neutral: "border-border-strong bg-subtle",
};

const CHIP_CLASS: Record<CardChip["tone"], string> = {
  outline: "border border-border-strong text-fg-2",
  direct: "bg-direct-soft text-direct-text",
  route: "bg-route-soft text-route-text",
  stack: "bg-stack-soft text-stack-text",
  success: "bg-success-soft text-success-text",
  warning: "bg-warning-soft text-warning-text",
  danger: "bg-danger-soft text-danger-text",
};

export function CardChipBadge({ chip }: { chip: CardChip }) {
  return (
    <span
      title={chip.title}
      className={cn(
        "inline-flex h-[18px] shrink-0 items-center whitespace-nowrap rounded-full px-1.5 text-badge",
        CHIP_CLASS[chip.tone],
        chip.title && "cursor-help",
      )}
    >
      {chip.label}
    </span>
  );
}

function HealthChip({
  consecutiveFailures,
  isHealthy,
}: {
  consecutiveFailures: number;
  isHealthy?: boolean;
}) {
  const { t } = useTranslation();
  const chip: CardChip =
    consecutiveFailures === 0
      ? { key: "health", label: t("health.operational"), tone: "success" }
      : isHealthy !== false
        ? { key: "health", label: t("health.degraded"), tone: "warning" }
        : { key: "health", label: t("health.circuitOpen"), tone: "danger" };
  return <CardChipBadge chip={chip} />;
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

const extractApiUrl = (provider: Provider, fallbackText: string) => {
  if (provider.notes?.trim()) {
    return provider.notes.trim();
  }

  if (provider.websiteUrl) {
    return provider.websiteUrl;
  }

  const config = provider.settingsConfig;

  if (config && typeof config === "object") {
    const object = config as Record<string, any>;
    const envBase =
      object?.env?.ANTHROPIC_BASE_URL || object?.env?.GOOGLE_GEMINI_BASE_URL;
    if (typeof envBase === "string" && envBase.trim()) {
      return envBase;
    }

    const directBaseUrl =
      object.baseUrl ||
      object.base_url ||
      object.options?.baseURL ||
      (Array.isArray(object.models)
        ? object.models.find(
            (model: unknown) =>
              model &&
              typeof model === "object" &&
              typeof (model as Record<string, unknown>).baseUrl === "string",
          )?.baseUrl
        : undefined);
    if (typeof directBaseUrl === "string" && directBaseUrl.trim()) {
      return directBaseUrl;
    }

    const baseUrl = object.config;

    if (typeof baseUrl === "string" && baseUrl.includes("base_url")) {
      const extractedBaseUrl = extractCodexBaseUrl(baseUrl);
      if (extractedBaseUrl) {
        return extractedBaseUrl;
      }
    }
  }

  return fallbackText;
};

export function ProviderCard({
  provider,
  appId,
  presentation,
  isCurrent,
  isInConfig = true,
  onEdit,
  onDelete,
  onConfigureUsage,
  onOpenWebsite,
  onDuplicate,
  onTest,
  onOpenTerminal,
  isTesting,
  dragHandleProps,
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

  const { data: health } = useProviderHealth(
    provider.id,
    appId,
    Boolean(presentation.showHealth) && isProxyAppId(appId),
  );

  const fallbackUrlText = t("provider.notConfigured", {
    defaultValue: "未配置接口地址",
  });

  const displayUrl = useMemo(() => {
    return extractApiUrl(provider, fallbackUrlText);
  }, [provider, fallbackUrlText]);

  const isClickableUrl = useMemo(() => {
    if (provider.notes?.trim()) {
      return false;
    }
    if (displayUrl === fallbackUrlText) {
      return false;
    }
    return true;
  }, [provider.notes, displayUrl, fallbackUrlText]);

  const isBoundCodexOfficial = codexOfficialIdentity === "managed_account";
  const usageEnabled =
    provider.meta?.usage_script?.enabled ?? isBoundCodexOfficial;
  const isOfficial = isOfficialProvider(provider, appId);
  const supportsOfficialSubscription =
    isOfficial && ["claude", "codex", "gemini", "grokbuild"].includes(appId);
  const isOfficialSubscriptionUsage =
    provider.meta?.usage_script?.templateType ===
    TEMPLATE_TYPES.OFFICIAL_SUBSCRIPTION;
  const officialSubscriptionEnabled =
    supportsOfficialSubscription && usageEnabled && isOfficialSubscriptionUsage;

  const isCopilot =
    provider.meta?.providerType === PROVIDER_TYPES.GITHUB_COPILOT ||
    provider.meta?.usage_script?.templateType === "github_copilot";
  const isCodexOauth =
    appId === "codex"
      ? isBoundCodexOfficial
      : provider.meta?.providerType === PROVIDER_TYPES.CODEX_OAUTH;
  // xAI OAuth (SuperGrok 反代)：额度经自管 OAuth token 自动显示，与 codex_oauth 同构
  const isXaiOauth = provider.meta?.providerType === PROVIDER_TYPES.XAI_OAUTH;
  // 获取用量数据以判断是否有多套餐
  // 累加模式应用：使用 isInConfig 代替 isCurrent
  const shouldAutoQuery = isAdditiveAppId(appId) ? isInConfig : isCurrent;
  const autoQueryInterval = shouldAutoQuery
    ? provider.meta?.usage_script?.autoQueryInterval || 0
    : 0;

  // 脚本用量只在「已启用 + 非官方 + 非官方订阅模板」时才查询；展开判定必须复用同一谓词，
  // 因为禁用的 React Query observer 仍会返回同 key 的旧缓存。
  const scriptUsageActive =
    usageEnabled && !isOfficial && !isOfficialSubscriptionUsage;
  const { data: usage } = useUsageQuery(provider.id, appId, {
    enabled: scriptUsageActive,
    autoQueryInterval,
  });

  const isTokenPlan =
    provider.meta?.usage_script?.templateType === "token_plan";
  // 官方订阅的额度窗口不能按普通多套餐展开；缓存残留的旧脚本结果同样不认。
  const hasMultiplePlans =
    scriptUsageActive &&
    !isTokenPlan &&
    usage?.success &&
    usage.data &&
    usage.data.length > 1;

  const [isExpanded, setIsExpanded] = useState(false);
  const expandLabel = isExpanded
    ? t("usage.collapse", { defaultValue: "收起" })
    : t("usage.expand", { defaultValue: "展开" });

  useEffect(() => {
    if (hasMultiplePlans) {
      setIsExpanded(true);
    }
  }, [hasMultiplePlans]);

  const handleOpenWebsite = () => {
    if (!isClickableUrl) {
      return;
    }
    onOpenWebsite(displayUrl);
  };
  return (
    <div
      className={cn(
        "group relative rounded-panel border transition-[border-color,box-shadow,background-color] duration-150",
        presentation.tone
          ? TONE_CLASS[presentation.tone]
          : "border-border bg-surface hover:border-border-strong hover:shadow-v7-sm",
        dragHandleProps?.isDragging && "z-10 cursor-grabbing shadow-v7-md",
      )}
    >
      <div className="flex min-h-[58px] items-center gap-3 py-2.5 pe-3 ps-2">
        {dragHandleProps ? (
          <button
            type="button"
            className={cn(
              "flex h-6 w-4 shrink-0 cursor-grab items-center justify-center text-fg-3 opacity-0 transition-opacity group-hover:opacity-100 focus-visible:opacity-100 active:cursor-grabbing",
              dragHandleProps.isDragging && "cursor-grabbing opacity-100",
            )}
            aria-label={t("provider.dragHandle")}
            {...dragHandleProps.attributes}
            {...dragHandleProps.listeners}
          >
            <GripVertical className="h-4 w-4" />
          </button>
        ) : (
          <span className="w-4 shrink-0" />
        )}

        <div
          className={cn(
            // 底色固定为白，图标颜色也要固定：单色（currentColor）图标和首字母
            // fallback 不能继承深色模式下的浅色文字
            "flex h-8 w-8 shrink-0 items-center justify-center rounded-[8px] border border-border bg-white text-neutral-900",
            presentation.dim && "opacity-60",
          )}
        >
          <ProviderIcon
            icon={resolveProviderIcon(appId, provider.icon, provider.iconColor)}
            name={provider.name}
            color={provider.iconColor}
            size={20}
            fallbackClassName="bg-transparent text-neutral-600"
          />
        </div>

        <div className="min-w-0 flex-1">
          <div className="flex min-w-0 items-center gap-1.5">
            <h3
              className={cn(
                "m-0 min-w-0 truncate text-strong",
                presentation.dim ? "text-fg-2" : "text-fg-1",
              )}
              title={provider.name}
            >
              {provider.name}
            </h3>
            {presentation.chips.map((chip) => (
              <CardChipBadge key={chip.key} chip={chip} />
            ))}
            {presentation.showHealth && health && (
              <HealthChip
                consecutiveFailures={health.consecutive_failures}
                isHealthy={health.is_healthy}
              />
            )}
          </div>

          {codexOfficialIdentity && codexOfficialIdentity !== "api_key" ? (
            <div className="mt-0.5 flex min-w-0 items-center gap-2 text-caption text-fg-2">
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
                    <span className="inline-flex shrink-0 items-center gap-1 text-warning-text">
                      <AlertTriangle className="h-3.5 w-3.5" />
                      {t("codexOauth.reauthBadge", "需要重新登录")}
                    </span>
                  )}
                </>
              ) : isCodexAuthStatusError ? (
                <span className="inline-flex min-w-0 items-center gap-1 text-warning-text">
                  <AlertTriangle className="h-3.5 w-3.5 shrink-0" />
                  <span className="truncate">
                    {t("codex.accountStatusUnavailable", {
                      defaultValue: "无法读取账号信息",
                    })}
                  </span>
                </span>
              ) : isCodexAuthStatusSuccess ? (
                <>
                  <span className="inline-flex min-w-0 items-center gap-1 text-sm text-warning-text">
                    <AlertTriangle className="h-3.5 w-3.5 shrink-0" />
                    <span className="truncate">
                      {t("codex.boundAccountUnavailable", {
                        defaultValue: "绑定的账号不可用",
                      })}
                    </span>
                  </span>
                  <button
                    type="button"
                    className="shrink-0 text-body font-medium text-action-text hover:underline"
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
          ) : displayUrl ? (
            <button
              type="button"
              onClick={handleOpenWebsite}
              className={cn(
                "mt-0.5 inline-flex max-w-full items-center overflow-hidden text-start text-caption text-fg-2",
                isClickableUrl
                  ? "cursor-pointer decoration-border-strong underline-offset-[3px] hover:underline"
                  : "cursor-default",
              )}
              title={displayUrl}
              disabled={!isClickableUrl}
            >
              <span className="min-w-0 truncate">{displayUrl}</span>
            </button>
          ) : null}
        </div>

        <div className="flex shrink-0 items-center gap-3">
          {/* 额度列 136 宽，带重置倒计时时 194 宽（见 QuotaLines） */}
          <div className="max-w-[208px] text-end">
            <div className="flex items-center justify-end gap-1">
              {isCopilot ? (
                <CopilotQuotaFooter
                  meta={provider.meta}
                  inline={true}
                  isCurrent={isCurrent}
                />
              ) : isCodexOauth ? (
                !isBoundCodexOfficial || usageEnabled ? (
                  <CodexOauthQuotaFooter
                    meta={provider.meta}
                    inline={true}
                    isCurrent={isCurrent}
                    autoQueryInterval={
                      isBoundCodexOfficial
                        ? (provider.meta?.usage_script?.autoQueryInterval ?? 5)
                        : undefined
                    }
                  />
                ) : null
              ) : isXaiOauth ? (
                <XaiOauthQuotaFooter
                  meta={provider.meta}
                  inline={true}
                  isCurrent={isCurrent}
                />
              ) : isOfficial ? (
                officialSubscriptionEnabled ? (
                  <SubscriptionQuotaFooter
                    appId={appId}
                    inline={true}
                    isCurrent={isCurrent}
                    autoQueryInterval={
                      provider.meta?.usage_script?.autoQueryInterval ?? 0
                    }
                  />
                ) : null
              ) : hasMultiplePlans ? (
                <span className="text-caption text-fg-2">
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
                <HoverTip content={expandLabel}>
                  <button
                    onClick={(e) => {
                      e.stopPropagation();
                      setIsExpanded(!isExpanded);
                    }}
                    className="shrink-0 rounded-control p-1 text-fg-3 transition-colors hover:bg-subtle hover:text-fg-1"
                    aria-label={expandLabel}
                    aria-expanded={isExpanded}
                  >
                    {isExpanded ? (
                      <ChevronUp size={14} />
                    ) : (
                      <ChevronDown size={14} />
                    )}
                  </button>
                </HoverTip>
              )}
            </div>
          </div>
          <ProviderCardActions
            providerName={provider.name}
            presentation={presentation}
            onEdit={() => onEdit(provider)}
            onDelete={() => onDelete(provider)}
            onDuplicate={onDuplicate ? () => onDuplicate(provider) : undefined}
            onTest={
              // 连通检测对第三方/自定义/Copilot/Codex-OAuth 供应商开放。官方供应商一律不给：
              // 它们 base_url 故意留空、走客户端默认/OAuth 端点，没有可靠的探测目标。
              onTest && appId !== "mcode" && provider.category !== "official"
                ? () => onTest(provider)
                : undefined
            }
            isTesting={isTesting}
            onConfigureUsage={
              (isOfficial && !supportsOfficialSubscription) ||
              isCopilot ||
              (isCodexOauth && !isBoundCodexOfficial) ||
              isXaiOauth
                ? undefined
                : () => onConfigureUsage(provider)
            }
            onOpenTerminal={
              onOpenTerminal ? () => onOpenTerminal(provider) : undefined
            }
          />
        </div>
      </div>

      {isExpanded && hasMultiplePlans && (
        <div className="mx-4 mb-3 border-t border-border pt-3">
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
    </div>
  );
}
