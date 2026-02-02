import { useMemo, useState, useRef, useEffect, useCallback } from "react";
import { createPortal } from "react-dom";
import { GripVertical, Key, Globe, Cpu, Building2, Info, Copy, Check } from "lucide-react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import type {
  DraggableAttributes,
  DraggableSyntheticListeners,
} from "@dnd-kit/core";
import type { Provider, ProviderCategory } from "@/types";
import type { AppId } from "@/lib/api";
import { cn } from "@/lib/utils";
import { ProviderActions } from "@/components/providers/ProviderActions";
import { ProviderIcon } from "@/components/ProviderIcon";
import { ProviderHealthBadge } from "@/components/providers/ProviderHealthBadge";
import { FailoverPriorityBadge } from "@/components/providers/FailoverPriorityBadge";
import { useProviderHealth } from "@/lib/query/failover";

// 可复制值组件 - 抽离到组件外部避免重复创建
function CopyableValue({ 
  value, 
  displayValue,
  className,
  onCopy 
}: { 
  value: string; 
  displayValue?: string;
  className?: string;
  onCopy: (value: string) => Promise<void>;
}) {
  const [copied, setCopied] = useState(false);
  const timeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  
  // 清理 timeout
  useEffect(() => {
    return () => {
      if (timeoutRef.current) {
        clearTimeout(timeoutRef.current);
      }
    };
  }, []);
  
  const handleCopy = async (e: React.MouseEvent) => {
    e.stopPropagation();
    try {
      await onCopy(value);
      setCopied(true);
      if (timeoutRef.current) {
        clearTimeout(timeoutRef.current);
      }
      timeoutRef.current = setTimeout(() => setCopied(false), 2000);
    } catch {
      // onCopy 内部已处理错误
    }
  };

  return (
    <button
      type="button"
      onClick={handleCopy}
      className={cn(
        "group/copy inline-flex items-center gap-1 hover:bg-muted/50 rounded px-1 -mx-1 transition-colors cursor-pointer text-left",
        className
      )}
    >
      <span className="break-all">{displayValue || value}</span>
      {copied ? (
        <Check className="h-3 w-3 flex-shrink-0 text-emerald-500" />
      ) : (
        <Copy className="h-3 w-3 flex-shrink-0 opacity-0 group-hover/copy:opacity-100 transition-opacity text-muted-foreground" />
      )}
    </button>
  );
}

interface DragHandleProps {
  attributes: DraggableAttributes;
  listeners: DraggableSyntheticListeners;
  isDragging: boolean;
}

interface ProviderCardCompactProps {
  provider: Provider;
  isCurrent: boolean;
  appId: AppId;
  isInConfig?: boolean;
  onSwitch: (provider: Provider) => void;
  onEdit: (provider: Provider) => void;
  onDelete: (provider: Provider) => void;
  onRemoveFromConfig?: (provider: Provider) => void;
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
  // 匿名模式
  isAnonymousMode?: boolean;
}

export function ProviderCardCompact({
  provider,
  isCurrent,
  appId,
  isInConfig = true,
  onSwitch,
  onEdit,
  onDelete,
  onRemoveFromConfig,
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
  isAnonymousMode = false,
}: ProviderCardCompactProps) {
  const { t } = useTranslation();
  const { data: health } = useProviderHealth(provider.id, appId);

  // 从 settingsConfig 提取关键配置信息 - 必须在 displayUrl 之前定义
  const configInfo = useMemo(() => {
    const config = provider.settingsConfig;
    if (!config || typeof config !== "object") return null;

    // Claude 配置结构: { env: { ANTHROPIC_BASE_URL, ANTHROPIC_AUTH_TOKEN, ANTHROPIC_MODEL, ... } }
    const env = (config as { env?: Record<string, string> }).env;
    if (env && typeof env === "object") {
      const baseUrl = env.ANTHROPIC_BASE_URL || env.GOOGLE_GEMINI_BASE_URL;
      const apiKey = env.ANTHROPIC_AUTH_TOKEN || env.ANTHROPIC_API_KEY || env.GOOGLE_API_KEY;
      const model = env.ANTHROPIC_MODEL || env.GOOGLE_GEMINI_MODEL;
      const haikuModel = env.ANTHROPIC_DEFAULT_HAIKU_MODEL;
      const sonnetModel = env.ANTHROPIC_DEFAULT_SONNET_MODEL;
      const opusModel = env.ANTHROPIC_DEFAULT_OPUS_MODEL;
      return {
        baseUrl: baseUrl || null,
        baseUrlHost: baseUrl ? (() => { try { return new URL(baseUrl).host; } catch { return baseUrl; } })() : null,
        hasApiKey: !!apiKey && apiKey.length > 0,
        apiKey: apiKey || null,
        apiKeyPreview: apiKey ? (apiKey.length > 12 ? `${apiKey.slice(0, 8)}...${apiKey.slice(-4)}` : apiKey.length > 4 ? `${apiKey.slice(0, 4)}...` : '***') : null,
        model: model || null,
        haikuModel: haikuModel || null,
        sonnetModel: sonnetModel || null,
        opusModel: opusModel || null,
      };
    }

    // Codex 配置结构: { auth?: string, config?: string }
    const codexConfig = config as { auth?: string; config?: string };
    if (codexConfig.config && typeof codexConfig.config === "string") {
      // 简单解析 TOML 中的 model 和 api_base_url
      const modelMatch = codexConfig.config.match(/model\s*=\s*"([^"]+)"/);
      const baseUrlMatch = codexConfig.config.match(/api_base_url\s*=\s*"([^"]+)"/);
      const baseUrl = baseUrlMatch ? baseUrlMatch[1] : null;
      return {
        baseUrl: baseUrl,
        baseUrlHost: baseUrl ? (() => { try { return new URL(baseUrl).host; } catch { return baseUrl; } })() : null,
        hasApiKey: !!codexConfig.auth && codexConfig.auth.length > 0,
        apiKey: codexConfig.auth || null,
        apiKeyPreview: codexConfig.auth ? (codexConfig.auth.length > 12 ? `${codexConfig.auth.slice(0, 8)}...${codexConfig.auth.slice(-4)}` : codexConfig.auth.length > 4 ? `${codexConfig.auth.slice(0, 4)}...` : '***') : null,
        model: modelMatch ? modelMatch[1] : null,
        haikuModel: null,
        sonnetModel: null,
        opusModel: null,
      };
    }

    return null;
  }, [provider.settingsConfig]);

  const displayUrl = useMemo(() => {
    if (provider.notes?.trim()) return provider.notes.trim();
    if (provider.websiteUrl) return provider.websiteUrl;
    // 如果没有官网和备注，尝试从配置中提取 base_url
    if (configInfo?.baseUrlHost) return configInfo.baseUrlHost;
    return null;
  }, [provider.notes, provider.websiteUrl, configInfo?.baseUrlHost]);

  // 用于点击跳转的完整 URL
  const clickableFullUrl = useMemo(() => {
    // 优先使用 websiteUrl
    if (provider.websiteUrl) return provider.websiteUrl;
    // 如果配置中的 baseUrl 是完整的 http/https URL，也可以点击
    if (configInfo?.baseUrl && /^https?:\/\//i.test(configInfo.baseUrl)) {
      return configInfo.baseUrl;
    }
    return null;
  }, [provider.websiteUrl, configInfo?.baseUrl]);

  const isClickableUrl = useMemo(() => {
    // 如果显示的是备注，不可点击
    if (provider.notes?.trim()) return false;
    // 有可点击的完整 URL 才可点击
    return !!clickableFullUrl;
  }, [provider.notes, clickableFullUrl]);

  // Tooltip 显示状态 - 支持 hover 和点击固定
  const [showTooltip, setShowTooltip] = useState(false);
  const [isPinned, setIsPinned] = useState(false); // 点击固定
  const containerRef = useRef<HTMLDivElement>(null);
  const tooltipRef = useRef<HTMLDivElement | null>(null);
  const [tooltipPos, setTooltipPos] = useState({ top: 0, left: 0 });
  const hideTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  
  // callback ref for tooltip
  const setTooltipRef = useCallback((node: HTMLDivElement | null) => {
    tooltipRef.current = node;
  }, []);

  // 清理 tooltipRef 当 tooltip 关闭时
  useEffect(() => {
    if (!showTooltip) {
      tooltipRef.current = null;
    }
  }, [showTooltip]);

  // 清理 hideTimeout
  useEffect(() => {
    return () => {
      if (hideTimeoutRef.current) {
        clearTimeout(hideTimeoutRef.current);
      }
    };
  }, []);

  // 延迟隐藏 tooltip，给用户时间移动到 tooltip 上
  const scheduleHide = useCallback(() => {
    if (isPinned) return;
    hideTimeoutRef.current = setTimeout(() => {
      setShowTooltip(false);
    }, 100);
  }, [isPinned]);

  const cancelHide = useCallback(() => {
    if (hideTimeoutRef.current) {
      clearTimeout(hideTimeoutRef.current);
      hideTimeoutRef.current = null;
    }
  }, []);

  const handleMouseEnter = useCallback(() => {
    if (isPinned) return;
    cancelHide();
    setShowTooltip(true);
  }, [isPinned, cancelHide]);

  const handleMouseLeave = useCallback(() => {
    if (isPinned) return;
    scheduleHide();
  }, [isPinned, scheduleHide]);

  // 计算 tooltip 位置
  useEffect(() => {
    if (showTooltip && containerRef.current) {
      const rect = containerRef.current.getBoundingClientRect();
      const tooltipWidth = 288;
      const tooltipHeight = 280;
      const viewportHeight = window.innerHeight;
      const viewportWidth = window.innerWidth;
      
      let left = rect.left;
      left = Math.max(8, Math.min(left, viewportWidth - tooltipWidth - 8));
      
      let top = rect.bottom + 4;
      if (top + tooltipHeight > viewportHeight - 8) {
        top = rect.top - tooltipHeight - 4;
      }
      
      setTooltipPos({ top, left });
    }
  }, [showTooltip]);

  // 监听滚动和点击外部关闭固定的 tooltip
  useEffect(() => {
    if (!isPinned) return;

    const handleScroll = () => {
      setShowTooltip(false);
      setIsPinned(false);
    };

    const handleClickOutside = (e: MouseEvent) => {
      const target = e.target as Node;
      // 如果点击的不是 tooltip 内部或触发按钮，则关闭
      const isInsideContainer = containerRef.current?.contains(target);
      const isInsideTooltip = tooltipRef.current?.contains(target);
      if (!isInsideContainer && !isInsideTooltip) {
        setShowTooltip(false);
        setIsPinned(false);
      }
    };

    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        setShowTooltip(false);
        setIsPinned(false);
      }
    };

    // 监听滚动（捕获阶段，确保能捕获到所有滚动）
    window.addEventListener('scroll', handleScroll, true);
    document.addEventListener('click', handleClickOutside, true);
    document.addEventListener('keydown', handleKeyDown);

    return () => {
      window.removeEventListener('scroll', handleScroll, true);
      document.removeEventListener('click', handleClickOutside, true);
      document.removeEventListener('keydown', handleKeyDown);
    };
  }, [isPinned]);

  // 分类标签映射
  const categoryLabel = useMemo(() => {
    const categoryMap: Record<ProviderCategory, string> = {
      official: t("provider.category.official", { defaultValue: "官方" }),
      cn_official: t("provider.category.cnOfficial", { defaultValue: "国产" }),
      aggregator: t("provider.category.aggregator", { defaultValue: "聚合" }),
      third_party: t("provider.category.thirdParty", { defaultValue: "第三方" }),
      custom: t("provider.category.custom", { defaultValue: "自定义" }),
    };
    return provider.category ? categoryMap[provider.category] : null;
  }, [provider.category, t]);

  const isActiveProvider =
    appId === "opencode"
      ? false
      : isAutoFailoverEnabled
        ? activeProviderId === provider.id
        : isCurrent;

  // 复制到剪贴板
  const copyToClipboard = useCallback(async (text: string) => {
    try {
      await navigator.clipboard.writeText(text);
      toast.success(t("common.copied", { defaultValue: "已复制" }));
    } catch {
      toast.error(t("common.copyFailed", { defaultValue: "复制失败" }));
    }
  }, [t]);

  const shouldUseGreen = isProxyTakeover && isActiveProvider;
  const shouldUseBlue = !isProxyTakeover && isActiveProvider;

  return (
    <div
      className={cn(
        "relative overflow-hidden rounded-xl border border-border/60 p-4 transition-all duration-300",
        "bg-card text-card-foreground group h-full flex flex-col",
        // Hover 效果：轻微上浮 + 柔和阴影
        "hover:-translate-y-1 hover:shadow-lg hover:shadow-gray-200/50 dark:hover:shadow-gray-900/30",
        isAutoFailoverEnabled || isProxyTakeover
          ? "hover:border-emerald-500/40"
          : "hover:border-gray-300 dark:hover:border-gray-600",
        shouldUseGreen && "border-emerald-500/50 shadow-sm shadow-emerald-500/10",
        shouldUseBlue && "border-blue-500/50 shadow-sm shadow-blue-500/10",
        dragHandleProps?.isDragging && "cursor-grabbing border-primary shadow-xl scale-105 z-[100]"
      )}
      onDoubleClick={() => onEdit(provider)}
    >
      {/* Background gradient */}
      <div
        className={cn(
          "absolute inset-0 bg-gradient-to-r to-transparent transition-opacity duration-500 pointer-events-none",
          shouldUseGreen && "from-emerald-500/10",
          shouldUseBlue && "from-blue-500/10",
          !isActiveProvider && "from-primary/10",
          isActiveProvider ? "opacity-100" : "opacity-0"
        )}
      />

      {/* Header: Icon + Name + Badges + Info */}
      <div className="relative flex items-center gap-2">
        {/* Drag handle */}
        <button
          type="button"
          className={cn(
            "-ml-1 flex-shrink-0 cursor-grab active:cursor-grabbing p-1",
            "text-muted-foreground/50 hover:text-muted-foreground transition-colors",
            dragHandleProps?.isDragging && "cursor-grabbing"
          )}
          aria-label={t("provider.dragHandle")}
          {...(dragHandleProps?.attributes ?? {})}
          {...(dragHandleProps?.listeners ?? {})}
        >
          <GripVertical className="h-3.5 w-3.5" />
        </button>

        {/* Icon */}
        <div className="h-8 w-8 rounded-lg bg-muted flex items-center justify-center border border-border flex-shrink-0 relative overflow-hidden">
          <ProviderIcon
            icon={provider.icon}
            name={provider.name}
            color={provider.iconColor}
            size={18}
          />
        </div>

        {/* Name and badges */}
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-1.5 flex-wrap">
            <h3 className="text-sm font-semibold leading-tight truncate">
              {provider.name}
            </h3>
            {provider.category === "third_party" && provider.meta?.isPartner && (
              <span
                className="text-yellow-500 dark:text-yellow-400 text-xs"
                title={t("provider.officialPartner", { defaultValue: "官方合作伙伴" })}
              >
                ⭐
              </span>
            )}
          </div>

          {/* Status badges */}
          <div className="flex items-center gap-1.5 mt-1">
            {isProxyRunning && isInFailoverQueue && health && (
              <ProviderHealthBadge consecutiveFailures={health.consecutive_failures} />
            )}
            {isAutoFailoverEnabled && isInFailoverQueue && failoverPriority && (
              <FailoverPriorityBadge priority={failoverPriority} />
            )}
          </div>
        </div>

        {/* Info icon - 右上角 */}
        {configInfo && (
          <div 
            ref={containerRef}
            className="flex-shrink-0"
            onMouseEnter={handleMouseEnter}
            onMouseLeave={handleMouseLeave}
          >
            <button
              type="button"
              className={cn(
                "flex items-center justify-center h-5 w-5 rounded transition-colors",
                isPinned ? "bg-primary/20 text-primary" : "hover:bg-muted"
              )}
              onClick={(e) => {
                e.stopPropagation();
                cancelHide(); // 取消任何待执行的隐藏
                if (isPinned) {
                  setIsPinned(false);
                  setShowTooltip(false);
                } else {
                  setIsPinned(true);
                  setShowTooltip(true);
                }
              }}
            >
              <Info className={cn(
                "h-3.5 w-3.5",
                isPinned ? "text-primary" : "text-muted-foreground hover:text-foreground"
              )} />
            </button>

            {/* Hover Tooltip */}
            {showTooltip && createPortal(
              <div 
                ref={setTooltipRef}
                className="fixed z-[9999] w-72 p-3 rounded-lg border border-border bg-popover text-popover-foreground shadow-xl text-xs select-text"
                style={{ top: tooltipPos.top, left: tooltipPos.left }}
                onMouseEnter={handleMouseEnter}
                onMouseLeave={handleMouseLeave}
              >
                <div className="space-y-2">
                  {/* API Key */}
                  <div className="flex items-start gap-2">
                    <Key className={cn("h-3.5 w-3.5 mt-0.5 flex-shrink-0", configInfo.hasApiKey ? "text-emerald-500" : "text-amber-500")} />
                    <div className="flex-1 min-w-0">
                      <div className="font-medium text-foreground">API Key</div>
                      <div className={cn("mt-0.5", configInfo.hasApiKey ? "text-emerald-500" : "text-amber-500")}>
                        {isAnonymousMode 
                          ? (configInfo.hasApiKey ? "••••••••" : t("provider.notConfigured", { defaultValue: "未配置" }))
                          : (configInfo.hasApiKey && configInfo.apiKey
                              ? <CopyableValue 
                                  value={configInfo.apiKey} 
                                  displayValue={configInfo.apiKeyPreview || undefined}
                                  className="text-emerald-500" 
                                  onCopy={copyToClipboard}
                                />
                              : t("provider.notConfigured", { defaultValue: "未配置" })
                            )
                        }
                      </div>
                    </div>
                  </div>

                  {/* Base URL */}
                  {configInfo.baseUrl && (
                    <div className="flex items-start gap-2">
                      <Globe className="h-3.5 w-3.5 mt-0.5 flex-shrink-0 text-blue-500" />
                      <div className="flex-1 min-w-0">
                        <div className="font-medium text-foreground">Base URL</div>
                        <div className="mt-0.5 text-muted-foreground">
                          {isAnonymousMode 
                            ? "••••••••.com" 
                            : <CopyableValue value={configInfo.baseUrl} onCopy={copyToClipboard} />
                          }
                        </div>
                      </div>
                    </div>
                  )}

                  {/* 模型配置 */}
                  {configInfo.model && (
                    <div className="flex items-start gap-2">
                      <Cpu className="h-3.5 w-3.5 mt-0.5 flex-shrink-0 text-purple-500" />
                      <div className="flex-1 min-w-0">
                        <div className="font-medium text-foreground">{t("provider.model", { defaultValue: "模型" })}</div>
                        <div className="mt-0.5 text-muted-foreground">
                          <CopyableValue value={configInfo.model} onCopy={copyToClipboard} />
                        </div>
                        {/* 显示其他模型配置 */}
                        {(configInfo.haikuModel || configInfo.sonnetModel || configInfo.opusModel) && (
                          <div className="mt-1 pt-1 border-t border-border/50 space-y-0.5 text-[10px]">
                            {configInfo.haikuModel && (
                              <div className="flex items-center">
                                <span className="text-muted-foreground/70 mr-1">Haiku:</span>
                                <CopyableValue value={configInfo.haikuModel} onCopy={copyToClipboard} />
                              </div>
                            )}
                            {configInfo.sonnetModel && (
                              <div className="flex items-center">
                                <span className="text-muted-foreground/70 mr-1">Sonnet:</span>
                                <CopyableValue value={configInfo.sonnetModel} onCopy={copyToClipboard} />
                              </div>
                            )}
                            {configInfo.opusModel && (
                              <div className="flex items-center">
                                <span className="text-muted-foreground/70 mr-1">Opus:</span>
                                <CopyableValue value={configInfo.opusModel} onCopy={copyToClipboard} />
                              </div>
                            )}
                          </div>
                        )}
                      </div>
                    </div>
                  )}

                  {/* 分类 */}
                  {categoryLabel && (
                    <div className="flex items-start gap-2">
                      <Building2 className="h-3.5 w-3.5 mt-0.5 flex-shrink-0 text-gray-500" />
                      <div className="flex-1 min-w-0">
                        <div className="font-medium text-foreground">{t("provider.categoryLabel", { defaultValue: "分类" })}</div>
                        <div className={cn(
                          "mt-0.5",
                          provider.category === "official" && "text-blue-500",
                          provider.category === "cn_official" && "text-emerald-500",
                          provider.category === "aggregator" && "text-purple-500",
                          provider.category === "third_party" && "text-amber-500",
                          provider.category === "custom" && "text-gray-500"
                        )}>
                          {categoryLabel}
                        </div>
                      </div>
                    </div>
                  )}

                  {/* 创建时间 */}
                  {provider.createdAt && (
                    <div className="pt-1 border-t border-border/50 text-[10px] text-muted-foreground/70">
                      {t("provider.createdAt", { defaultValue: "添加于" })}: {new Date(provider.createdAt).toLocaleDateString()}
                    </div>
                  )}
                </div>
              </div>,
              document.body
            )}
          </div>
        )}
      </div>

      {/* URL/Notes - truncated with subtle styling */}
      {displayUrl && (
        <button
          type="button"
          onClick={() => isClickableUrl && !isAnonymousMode && clickableFullUrl && onOpenWebsite(clickableFullUrl)}
          className={cn(
            "relative mt-2.5 text-xs max-w-full truncate text-left transition-colors",
            isAnonymousMode
              ? "text-muted-foreground/40 cursor-default"
              : isClickableUrl
                ? "text-gray-500 dark:text-gray-400 hover:text-blue-500 dark:hover:text-blue-400 hover:underline cursor-pointer"
                : "text-gray-400 dark:text-gray-500 cursor-default"
          )}
          title={isAnonymousMode ? t("provider.hiddenInAnonymousMode", { defaultValue: "隐私模式下已隐藏" }) : displayUrl}
          disabled={!isClickableUrl || isAnonymousMode}
        >
          {isAnonymousMode ? "••••••••.com/••••" : displayUrl}
        </button>
      )}

      {/* Spacer */}
      <div className="flex-1 min-h-2" />

      {/* Actions - isolated bottom area with subtle background */}
      <div className="relative mt-3 pt-3 px-1 -mx-1 border-t border-gray-100 dark:border-gray-800 bg-gray-50/50 dark:bg-gray-900/30 rounded-b-lg flex items-center justify-end">
        <ProviderActions
          appId={appId}
          isCurrent={isCurrent}
          isInConfig={isInConfig}
          isTesting={isTesting}
          isProxyTakeover={isProxyTakeover}
          onSwitch={() => onSwitch(provider)}
          onEdit={() => onEdit(provider)}
          onDuplicate={() => onDuplicate(provider)}
          onTest={onTest ? () => onTest(provider) : undefined}
          onConfigureUsage={() => onConfigureUsage(provider)}
          onDelete={() => onDelete(provider)}
          onRemoveFromConfig={onRemoveFromConfig ? () => onRemoveFromConfig(provider) : undefined}
          onOpenTerminal={onOpenTerminal ? () => onOpenTerminal(provider) : undefined}
          isAutoFailoverEnabled={isAutoFailoverEnabled}
          isInFailoverQueue={isInFailoverQueue}
          onToggleFailover={onToggleFailover}
          compact
        />
      </div>
    </div>
  );
}

export default ProviderCardCompact;
