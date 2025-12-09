import { useMemo } from "react";
import { GripVertical } from "lucide-react";
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
import { Switch } from "@/components/ui/switch";
import { Label } from "@/components/ui/label";
import { ProviderHealthBadge } from "@/components/providers/ProviderHealthBadge";
import {
  useProviderHealth,
  useResetCircuitBreaker,
  useSetProxyTarget,
} from "@/lib/query/failover";
import { toast } from "sonner";

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
  isTesting?: boolean;
  isProxyRunning: boolean;
  proxyPriority?: number; // 代理目标的实际优先级 (1, 2, 3...)
  allProviders?: Provider[]; // 所有供应商列表，用于计算开启后的优先级
  dragHandleProps?: DragHandleProps;
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
  isTesting,
  isProxyRunning,
  proxyPriority,
  allProviders,
  dragHandleProps,
}: ProviderCardProps) {
  const { t } = useTranslation();

  // 获取供应商健康状态
  const { data: health } = useProviderHealth(provider.id, appId);

  // 设置代理目标
  const setProxyTargetMutation = useSetProxyTarget();

  // 重置熔断器
  const resetCircuitBreaker = useResetCircuitBreaker();

  const handleSetProxyTarget = async (enabled: boolean) => {
    try {
      await setProxyTargetMutation.mutateAsync({
        providerId: provider.id,
        appType: appId,
        enabled,
      });

      // 计算实际优先级（开启时）
      let actualPriority: number | undefined;
      if (enabled && allProviders) {
        // 模拟开启后的状态：获取所有将要启用代理的 providers
        const futureProxyTargets = allProviders.filter((p) => {
          // 包括：已经是代理目标的 或 当前要开启的这个
          if (p.id === provider.id) return true;
          return p.isProxyTarget;
        });

        // 按 sortIndex 排序
        const sortedTargets = futureProxyTargets.sort((a, b) => {
          const indexA = a.sortIndex ?? Number.MAX_SAFE_INTEGER;
          const indexB = b.sortIndex ?? Number.MAX_SAFE_INTEGER;
          return indexA - indexB;
        });

        // 找到当前 provider 的位置
        const position = sortedTargets.findIndex((p) => p.id === provider.id);
        actualPriority = position >= 0 ? position + 1 : undefined;
      }

      const message = enabled
        ? actualPriority
          ? t("provider.proxyTargetEnabled", {
              defaultValue: `已启用代理目标（优先级：P${actualPriority}）`,
            })
          : t("provider.proxyTargetEnabled", {
              defaultValue: "已启用代理目标",
            })
        : t("provider.proxyTargetDisabled", {
            defaultValue: "已禁用代理目标",
          });

      const description = enabled
        ? t("provider.proxyTargetEnabledDesc", {
            defaultValue: "下次请求将按优先级自动选择此供应商",
          })
        : t("provider.proxyTargetDisabledDesc", {
            defaultValue: "后续请求将使用其他可用供应商",
          });

      toast.success(message, {
        description,
        duration: 4000,
      });
    } catch (error) {
      toast.error(
        t("provider.setProxyTargetFailed", {
          defaultValue: "操作失败",
        }) +
          ": " +
          String(error),
      );
    }
  };

  const handleResetCircuitBreaker = async () => {
    try {
      await resetCircuitBreaker.mutateAsync({
        providerId: provider.id,
        appType: appId,
      });
      toast.success(
        t("provider.circuitBreakerReset", {
          defaultValue: "熔断器已重置",
        }),
      );
    } catch (error) {
      toast.error(
        t("provider.circuitBreakerResetFailed", {
          defaultValue: "重置失败",
        }) +
          ": " +
          String(error),
      );
    }
  };

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

  const handleOpenWebsite = () => {
    if (!isClickableUrl) {
      return;
    }
    onOpenWebsite(displayUrl);
  };

  return (
    <div
      className={cn(
        "relative overflow-hidden rounded-xl border border-border p-4 transition-all duration-300",
        "bg-card text-card-foreground group hover:border-border-active",
        isCurrent ? "border-primary/50 shadow-sm" : "hover:shadow-sm",
        dragHandleProps?.isDragging &&
          "cursor-grabbing border-primary shadow-lg scale-105 z-10",
      )}
    >
      <div className="absolute inset-0 bg-gradient-to-r from-primary/10 to-transparent opacity-0 group-hover:opacity-100 transition-opacity duration-500 pointer-events-none" />
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
            <div className="flex flex-wrap items-center gap-2 min-h-[20px]">
              <h3 className="text-base font-semibold leading-none">
                {provider.name}
              </h3>

              {/* 健康状态徽章和优先级 */}
              {isProxyRunning && (
                <div className="flex items-center gap-1.5">
                  {/* 健康徽章：代理目标启用时始终显示，没有健康数据时默认为正常(0失败) */}
                  {(provider.isProxyTarget || health) && (
                    <ProviderHealthBadge
                      consecutiveFailures={health?.consecutive_failures ?? 0}
                      isProxyTarget={provider.isProxyTarget ?? false}
                    />
                  )}
                  {/* 优先级：仅在代理目标启用时显示 */}
                  {provider.isProxyTarget && proxyPriority && (
                    <span
                      className="text-xs text-muted-foreground"
                      title={`代理队列优先级：第${proxyPriority}位`}
                    >
                      P{proxyPriority}
                    </span>
                  )}
                </div>
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

              {/* 代理目标开关 - 仅在代理服务运行时显示 */}
              {isProxyRunning && (
                <div
                  className="flex items-center gap-2 ml-2"
                  onClick={(e) => e.stopPropagation()}
                >
                  <Switch
                    id={`proxy-target-switch-${provider.id}`}
                    checked={provider.isProxyTarget || false}
                    onCheckedChange={(checked) => {
                      handleSetProxyTarget(checked);
                    }}
                    disabled={setProxyTargetMutation.isPending}
                    className="scale-75 data-[state=checked]:bg-green-500"
                  />
                  {provider.isProxyTarget && (
                    <Label
                      htmlFor={`proxy-target-switch-${provider.id}`}
                      className="text-xs font-medium text-green-600 dark:text-green-400 cursor-pointer"
                    >
                      {t("provider.proxyTarget", { defaultValue: "代理目标" })}
                    </Label>
                  )}
                  {!provider.isProxyTarget && (
                    <Label
                      htmlFor={`proxy-target-switch-${provider.id}`}
                      className="text-xs text-muted-foreground cursor-pointer opacity-0 group-hover:opacity-100 transition-opacity"
                    >
                      {t("provider.setAsProxyTarget", {
                        defaultValue: "设为代理",
                      })}
                    </Label>
                  )}
                </div>
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

        <div className="relative flex items-center ml-auto">
          <div className="ml-auto transition-transform duration-200 group-hover:-translate-x-[12.25rem] group-focus-within:-translate-x-[12.25rem] sm:group-hover:-translate-x-[14.25rem] sm:group-focus-within:-translate-x-[14.25rem]">
            <UsageFooter
              provider={provider}
              providerId={provider.id}
              appId={appId}
              usageEnabled={usageEnabled}
              isCurrent={isCurrent}
              inline={true}
            />
          </div>

          <div className="absolute right-0 top-1/2 -translate-y-1/2 flex items-center gap-1.5 opacity-0 pointer-events-none group-hover:opacity-100 group-focus-within:opacity-100 group-hover:pointer-events-auto group-focus-within:pointer-events-auto transition-all duration-200 translate-x-2 group-hover:translate-x-0 group-focus-within:translate-x-0">
            <ProviderActions
              isCurrent={isCurrent}
              isTesting={isTesting}
              onSwitch={() => onSwitch(provider)}
              onEdit={() => onEdit(provider)}
              onDuplicate={() => onDuplicate(provider)}
              onTest={onTest ? () => onTest(provider) : undefined}
              onConfigureUsage={() => onConfigureUsage(provider)}
              onDelete={() => onDelete(provider)}
              onResetCircuitBreaker={
                isProxyRunning && provider.isProxyTarget
                  ? handleResetCircuitBreaker
                  : undefined
              }
              isProxyTarget={provider.isProxyTarget}
              consecutiveFailures={health?.consecutive_failures ?? 0}
            />
          </div>
        </div>
      </div>
    </div>
  );
}
