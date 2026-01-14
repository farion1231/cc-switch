import { useMemo, useState, useEffect, useRef } from "react";
import { GripVertical, ChevronDown, ChevronUp } from "lucide-react";
import { useTranslation } from "react-i18next";
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
import { ProviderHealthBadge } from "@/components/providers/ProviderHealthBadge";
import { FailoverPriorityBadge } from "@/components/providers/FailoverPriorityBadge";
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
  onSwitch: (provider: Provider) => void;
  onEdit: (provider: Provider) => void;
  onDelete: (provider: Provider) => void;
  onConfigureUsage: (provider: Provider) => void;
  onOpenWebsite: (url: string) => void;
  onDuplicate: (provider: Provider) => void;
  onTest?: (provider: Provider) => void;
  onOpenTerminal?: (provider: Provider) => void;
  isTesting?: boolean;
  isProxyRunning: boolean;
  isProxyTakeover?: boolean; // 代理接管模式（Live配置已被接管，切换为热切换）
  dragHandleProps?: DragHandleProps;
  // 故障转移相关
  isAutoFailoverEnabled?: boolean; // 是否开启自动故障转移
  failoverPriority?: number; // 故障转移优先级（1 = P1, 2 = P2, ...）
  isInFailoverQueue?: boolean; // 是否在故障转移队列中
  onToggleFailover?: (enabled: boolean) => void; // 切换故障转移队列
  activeProviderId?: string; // 代理当前实际使用的供应商 ID（用于故障转移模式下标注绿色边框）
}

const extractApiUrl = (provider: Provider, fallbackText: string) => {
  // 优先级 1: 备注
  if (provider.notes?.trim()) {
    return provider.notes.trim();
  }

  // 优先级 2: 官网地址
  if (provider.websiteUrl) {
    return provider.websiteUrl;
  }

  // 优先级 3: 从配置中提取请求地址
  const config = provider.settingsConfig;

  if (config && typeof config === "object") {
    const envBase =
      (config as Record<string, any>)?.env?.ANTHROPIC_BASE_URL ||
      (config as Record<string, any>)?.env?.GOOGLE_GEMINI_BASE_URL;
    if (typeof envBase === "string" && envBase.trim()) {
      return envBase;
    }

    const baseUrl = (config as Record<string, any>)?.config;

    if (typeof baseUrl === "string" && baseUrl.includes("base_url")) {
      const match = baseUrl.match(/base_url\s*=\s*['"]([^'"]+)['"]/);
      if (match?.[1]) {
        return match[1];
      }
    }
  }

  return fallbackText;
};

export function ProviderCard({
  provider,
  isCurrent,
  appId,
  onSwitch,
  onEdit,
  onDelete,
  onConfigureUsage,
  onOpenWebsite,
  onDuplicate,
  onTest,
  onOpenTerminal,
  isTesting,
  isProxyRunning,
  isProxyTakeover = false,
  dragHandleProps,
  // 故障转移相关
  isAutoFailoverEnabled = false,
  failoverPriority,
  isInFailoverQueue = false,
  onToggleFailover,
  activeProviderId,
}: ProviderCardProps) {
  const { t } = useTranslation();

  // 获取供应商健康状态
  const { data: health } = useProviderHealth(provider.id, appId);

  const fallbackUrlText = t("provider.notConfigured", {
    defaultValue: "未配置接口地址",
  });

  const displayUrl = useMemo(() => {
    return extractApiUrl(provider, fallbackUrlText);
  }, [provider, fallbackUrlText]);

  // 判断是否为可点击的 URL（备注不可点击）
  const isClickableUrl = useMemo(() => {
    // 如果有备注，则不可点击
    if (provider.notes?.trim()) {
      return false;
    }
    // 如果显示的是回退文本，也不可点击
    if (displayUrl === fallbackUrlText) {
      return false;
    }
    // 其他情况（官网地址或请求地址）可点击
    return true;
  }, [provider.notes, displayUrl, fallbackUrlText]);

  const usageEnabled = provider.meta?.usage_script?.enabled ?? false;

  // 获取用量数据以判断是否有多套餐
  const autoQueryInterval = isCurrent
    ? provider.meta?.usage_script?.autoQueryInterval || 0
    : 0;

  const { data: usage } = useUsageQuery(provider.id, appId, {
    enabled: usageEnabled,
    autoQueryInterval,
  });

  const hasMultiplePlans =
    usage?.success && usage.data && usage.data.length > 1;

  // 多套餐默认展开
  const [isExpanded, setIsExpanded] = useState(false);

  // 操作按钮容器 ref，用于动态计算宽度
  const actionsRef = useRef<HTMLDivElement>(null);
  const [actionsWidth, setActionsWidth] = useState(0);

  // 当检测到多套餐时自动展开
  useEffect(() => {
    if (hasMultiplePlans) {
      setIsExpanded(true);
    }
  }, [hasMultiplePlans]);

  // 动态获取操作按钮宽度
  useEffect(() => {
    if (actionsRef.current) {
      const updateWidth = () => {
        const width = actionsRef.current?.offsetWidth || 0;
        setActionsWidth(width);
      };
      updateWidth();
      // 监听窗口大小变化
      window.addEventListener("resize", updateWidth);
      return () => window.removeEventListener("resize", updateWidth);
    }
  }, [onTest, onOpenTerminal]); // 按钮数量可能变化时重新计算

  const handleOpenWebsite = () => {
    if (!isClickableUrl) {
      return;
    }
    onOpenWebsite(displayUrl);
  };

  // 判断是否是"当前使用中"的供应商
  // - 故障转移模式：代理实际使用的供应商（activeProviderId）
  // - 代理接管模式（非故障转移）：isCurrent
  // - 普通模式：isCurrent
  const isActiveProvider = isAutoFailoverEnabled
    ? activeProviderId === provider.id
    : isCurrent;

  // 判断是否使用绿色（代理接管模式）还是蓝色（普通模式）
  const shouldUseGreen = isProxyTakeover && isActiveProvider;
  const shouldUseBlue = !isProxyTakeover && isActiveProvider;

  return (
    <div
      className={cn(
        "relative overflow-hidden rounded-xl border border-border p-4 transition-all duration-300",
        "bg-card text-card-foreground group",
        // hover 时的边框效果
        isAutoFailoverEnabled || isProxyTakeover
          ? "hover:border-emerald-500/50"
          : "hover:border-border-active",
        // 当前激活的供应商边框样式
        shouldUseGreen &&
          "border-emerald-500/60 shadow-sm shadow-emerald-500/10",
        shouldUseBlue && "border-blue-500/60 shadow-sm shadow-blue-500/10",
        !isActiveProvider && "hover:shadow-sm",
        dragHandleProps?.isDragging &&
          "cursor-grabbing border-primary shadow-lg scale-105 z-10",
      )}
    >
      <div
        className={cn(
          "absolute inset-0 bg-gradient-to-r to-transparent transition-opacity duration-500 pointer-events-none",
          // 代理接管模式使用绿色渐变，普通模式使用蓝色渐变
          shouldUseGreen && "from-emerald-500/10",
          shouldUseBlue && "from-blue-500/10",
          !isActiveProvider && "from-primary/10",
          isActiveProvider ? "opacity-100" : "opacity-0",
        )}
      />
      <div className="relative flex flex-col gap-4 sm:flex-row sm:items-center sm:justify-between">
        <div className="flex flex-1 items-center gap-2">
          <button
            type="button"
            className={cn(
              "-ml-1.5 flex-shrink-0 cursor-grab active:cursor-grabbing p-1.5",
              "text-muted-foreground/50 hover:text-muted-foreground transition-colors",
              dragHandleProps?.isDragging && "cursor-grabbing",
            )}
            aria-label={t("provider.dragHandle")}
            {...(dragHandleProps?.attributes ?? {})}
            {...(dragHandleProps?.listeners ?? {})}
          >
            <GripVertical className="h-4 w-4" />
          </button>

          {/* 供应商图标 */}
          <div className="h-8 w-8 rounded-lg bg-muted flex items-center justify-center border border-border group-hover:scale-105 transition-transform duration-300">
            <ProviderIcon
              icon={provider.icon}
              name={provider.name}
              color={provider.iconColor}
              size={20}
            />
          </div>

          <div className="space-y-1">
            <div className="flex flex-wrap items-center gap-2 min-h-7">
              <h3 className="text-base font-semibold leading-none">
                {provider.name}
              </h3>

              {/* 健康状态徽章 */}
              {isProxyRunning && isInFailoverQueue && health && (
                <ProviderHealthBadge
                  consecutiveFailures={health.consecutive_failures}
                />
              )}

              {/* 故障转移优先级徽章 */}
              {isAutoFailoverEnabled &&
                isInFailoverQueue &&
                failoverPriority && (
                  <FailoverPriorityBadge priority={failoverPriority} />
                )}

              {provider.category === "third_party" &&
                provider.meta?.isPartner && (
                  <span
                    className="text-yellow-500 dark:text-yellow-400"
                    title={t("provider.officialPartner", {
                      defaultValue: "官方合作伙伴",
                    })}
                  >
                    ⭐
                  </span>
                )}
            </div>

            {displayUrl && (
              <button
                type="button"
                onClick={handleOpenWebsite}
                className={cn(
                  "inline-flex items-center text-sm max-w-[280px]",
                  isClickableUrl
                    ? "text-blue-500 transition-colors hover:underline dark:text-blue-400 cursor-pointer"
                    : "text-muted-foreground cursor-default",
                )}
                title={displayUrl}
                disabled={!isClickableUrl}
              >
                <span className="truncate">{displayUrl}</span>
              </button>
            )}
          </div>
        </div>

        <div
          className="relative flex items-center ml-auto min-w-0 gap-3"
          style={
            {
              "--actions-width": `${actionsWidth || 320}px`,
            } as React.CSSProperties
          }
        >
          {/* 用量信息区域 - hover 时向左移动，为操作按钮腾出空间 */}
          <div className="ml-auto">
            <div className="flex items-center gap-1 transition-transform duration-200 group-hover:-translate-x-[var(--actions-width)] group-focus-within:-translate-x-[var(--actions-width)]">
              {/* 多套餐时显示套餐数量，单套餐时显示详细信息 */}
              {hasMultiplePlans ? (
                <div className="flex items-center gap-2 text-xs text-gray-600 dark:text-gray-400">
                  <span className="font-medium">
                    {t("usage.multiplePlans", {
                      count: usage?.data?.length || 0,
                      defaultValue: `${usage?.data?.length || 0} 个套餐`,
                    })}
                  </span>
                </div>
              ) : (
                <UsageFooter
                  provider={provider}
                  providerId={provider.id}
                  appId={appId}
                  usageEnabled={usageEnabled}
                  isCurrent={isCurrent}
                  inline={true}
                />
              )}
              {/* 展开/折叠按钮 - 仅在有多套餐时显示 */}
              {hasMultiplePlans && (
                <button
                  onClick={(e) => {
                    e.stopPropagation();
                    setIsExpanded(!isExpanded);
                  }}
                  className="p-1 rounded hover:bg-gray-100 dark:hover:bg-gray-800 transition-colors text-gray-500 dark:text-gray-400 flex-shrink-0"
                  title={
                    isExpanded
                      ? t("usage.collapse", { defaultValue: "收起" })
                      : t("usage.expand", { defaultValue: "展开" })
                  }
                >
                  {isExpanded ? (
                    <ChevronUp size={14} />
                  ) : (
                    <ChevronDown size={14} />
                  )}
                </button>
              )}
            </div>
          </div>

          {/* 操作按钮区域 - 绝对定位在右侧，hover 时滑入，与用量信息保持间距 */}
          <div
            ref={actionsRef}
            className="absolute right-0 top-1/2 -translate-y-1/2 flex items-center gap-1.5 pl-3 opacity-0 pointer-events-none group-hover:opacity-100 group-focus-within:opacity-100 group-hover:pointer-events-auto group-focus-within:pointer-events-auto transition-all duration-200 translate-x-2 group-hover:translate-x-0 group-focus-within:translate-x-0"
          >
            <ProviderActions
              isCurrent={isCurrent}
              isTesting={isTesting}
              isProxyTakeover={isProxyTakeover}
              onSwitch={() => onSwitch(provider)}
              onEdit={() => onEdit(provider)}
              onDuplicate={() => onDuplicate(provider)}
              onTest={onTest ? () => onTest(provider) : undefined}
              onConfigureUsage={() => onConfigureUsage(provider)}
              onDelete={() => onDelete(provider)}
              onOpenTerminal={
                onOpenTerminal ? () => onOpenTerminal(provider) : undefined
              }
              // 故障转移相关
              isAutoFailoverEnabled={isAutoFailoverEnabled}
              isInFailoverQueue={isInFailoverQueue}
              onToggleFailover={onToggleFailover}
            />
          </div>
        </div>
      </div>

      {/* 展开的完整套餐列表 */}
      {isExpanded && hasMultiplePlans && (
        <div className="mt-4 pt-4 border-t border-border-default">
          <UsageFooter
            provider={provider}
            providerId={provider.id}
            appId={appId}
            usageEnabled={usageEnabled}
            isCurrent={isCurrent}
            inline={false}
          />
        </div>
      )}
    </div>
  );
}
