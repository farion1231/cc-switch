import {
  BarChart3,
  Check,
  Copy,
  Edit,
  Loader2,
  Minus,
  MoreHorizontal,
  Play,
  Plus,
  Terminal,
  TestTube2,
  Trash2,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { cn } from "@/lib/utils";
import type { AppId } from "@/lib/api";

interface ProviderActionsProps {
  appId?: AppId;
  isCurrent: boolean;
  /** OpenCode: 是否已添加到配置 */
  isInConfig?: boolean;
  /** OpenCode: 配置加载中 */
  isConfigLoading?: boolean;
  isTesting?: boolean;
  isProxyTakeover?: boolean;
  onSwitch: () => void;
  onEdit: () => void;
  onDuplicate: () => void;
  onTest?: () => void;
  onConfigureUsage?: () => void;
  onDelete: () => void;
  /** OpenCode: remove from live config (not delete from database) */
  onRemoveFromConfig?: () => void;
  onOpenTerminal?: () => void;
  // 故障转移相关
  isAutoFailoverEnabled?: boolean;
  isInFailoverQueue?: boolean;
  onToggleFailover?: (enabled: boolean) => void;
  /** 紧凑模式：用于卡片视图 */
  compact?: boolean;
}

export function ProviderActions({
  appId,
  isCurrent,
  isInConfig = false,
  isConfigLoading = false,
  isTesting,
  isProxyTakeover = false,
  onSwitch,
  onEdit,
  onDuplicate,
  onTest,
  onConfigureUsage,
  onDelete,
  onRemoveFromConfig,
  onOpenTerminal,
  // 故障转移相关
  isAutoFailoverEnabled = false,
  isInFailoverQueue = false,
  onToggleFailover,
  compact = false,
}: ProviderActionsProps) {
  const { t } = useTranslation();
  const iconButtonClass = cn(
    compact ? "h-6 w-6 p-0.5" : "h-8 w-8 p-1",
    "text-muted-foreground hover:text-foreground transition-colors",
  );

  // OpenCode 使用累加模式
  const isOpenCodeMode = appId === "opencode";

  // 故障转移模式下的按钮逻辑（OpenCode 不支持故障转移）
  const isFailoverMode =
    !isOpenCodeMode && isAutoFailoverEnabled && onToggleFailover;
  // 处理主按钮点击
  const handleMainButtonClick = () => {
    if (isOpenCodeMode) {
      // OpenCode 模式：切换配置状态（添加/移除）
      if (isInConfig) {
        // Use onRemoveFromConfig if available, otherwise fall back to onDelete
        if (onRemoveFromConfig) {
          onRemoveFromConfig();
        } else {
          onDelete();
        }
      } else {
        onSwitch(); // 添加到配置
      }
    } else if (isFailoverMode) {
      // 故障转移模式：切换队列状态
      onToggleFailover(!isInFailoverQueue);
    } else {
      // 普通模式：切换供应商
      onSwitch();
    }
  };

  // 主按钮的状态和样式
  const getMainButtonState = () => {
    // OpenCode 累加模式
    if (isOpenCodeMode) {
      if (isConfigLoading) {
        return {
          disabled: true,
          variant: "ghost" as const,
          className:
            "bg-gray-100 text-gray-400 dark:bg-gray-800/50 dark:text-gray-500 border border-gray-200 dark:border-gray-700",
          icon: <Loader2 className="h-4 w-4 animate-spin" />,
          text: t("common.loading"),
        };
      }
      if (isInConfig) {
        return {
          disabled: false,
          variant: "ghost" as const,
          className:
            "bg-orange-50 text-orange-600 hover:bg-orange-100 dark:bg-orange-500/10 dark:text-orange-400 dark:hover:bg-orange-500/20 border border-orange-200 dark:border-orange-800",
          icon: <Minus className="h-4 w-4" />,
          text: t("provider.removeFromConfig", { defaultValue: "移除" }),
        };
      }
      return {
        disabled: false,
        variant: "ghost" as const,
        className:
          "bg-emerald-50 text-emerald-600 hover:bg-emerald-100 dark:bg-emerald-500/10 dark:text-emerald-400 dark:hover:bg-emerald-500/20 border border-emerald-200 dark:border-emerald-800",
        icon: <Plus className="h-4 w-4" />,
        text: t("provider.addToConfig", { defaultValue: "添加" }),
      };
    }

    // 故障转移模式
    if (isFailoverMode) {
      if (isInFailoverQueue) {
        return {
          disabled: false,
          variant: "ghost" as const,
          className:
            "bg-blue-50 text-blue-600 hover:bg-blue-100 dark:bg-blue-500/10 dark:text-blue-400 dark:hover:bg-blue-500/20 border border-blue-200 dark:border-blue-800",
          icon: <Check className="h-4 w-4" />,
          text: t("failover.inQueue", { defaultValue: "已加入" }),
        };
      }
      return {
        disabled: false,
        variant: "ghost" as const,
        className:
          "bg-blue-50 text-blue-600 hover:bg-blue-100 dark:bg-blue-500/10 dark:text-blue-400 dark:hover:bg-blue-500/20 border border-blue-200 dark:border-blue-800",
        icon: <Plus className="h-4 w-4" />,
        text: t("failover.addQueue", { defaultValue: "加入" }),
      };
    }

    // 普通模式
    if (isCurrent) {
      return {
        disabled: true,
        variant: "ghost" as const,
        className:
          "bg-gray-100 text-gray-400 dark:bg-gray-800/50 dark:text-gray-500 border border-gray-200 dark:border-gray-700",
        icon: <Check className="h-4 w-4" />,
        text: t("provider.inUse"),
      };
    }

    return {
      disabled: false,
      variant: "ghost" as const,
      className: isProxyTakeover
        ? "bg-emerald-50 text-emerald-600 hover:bg-emerald-100 dark:bg-emerald-500/10 dark:text-emerald-400 dark:hover:bg-emerald-500/20 border border-emerald-200 dark:border-emerald-800"
        : "bg-blue-50 text-blue-600 hover:bg-blue-100 dark:bg-blue-500/10 dark:text-blue-400 dark:hover:bg-blue-500/20 border border-blue-200 dark:border-blue-800",
      icon: <Play className="h-4 w-4" />,
      text: t("provider.enable"),
    };
  };

  const buttonState = getMainButtonState();

  return (
    <div className={cn("flex items-center", compact ? "gap-1.5" : "gap-3")}>
      <Button
        size="sm"
        variant={buttonState.variant}
        onClick={handleMainButtonClick}
        disabled={buttonState.disabled}
        className={cn(
          compact ? "w-[3.5rem] px-1.5 h-7 text-xs" : "w-[4.5rem] px-2.5",
          "rounded-lg",
          buttonState.className,
        )}
      >
        {buttonState.icon}
        {!compact && buttonState.text}
      </Button>

      {/* 紧凑模式：常用操作 + 更多菜单 */}
      {compact ? (
        <div className="flex items-center gap-1">
          <Button
            size="icon"
            variant="ghost"
            onClick={onEdit}
            title={t("common.edit")}
            className={iconButtonClass}
          >
            <Edit className="h-3.5 w-3.5" />
          </Button>

          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button size="icon" variant="ghost" className={iconButtonClass}>
                <MoreHorizontal className="h-3.5 w-3.5" />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end" className="w-40">
              <DropdownMenuItem onClick={onDuplicate}>
                <Copy className="h-4 w-4 mr-2" />
                {t("provider.duplicate")}
              </DropdownMenuItem>
              {onTest && (
                <DropdownMenuItem onClick={onTest} disabled={isTesting}>
                  {isTesting ? (
                    <Loader2 className="h-4 w-4 mr-2 animate-spin" />
                  ) : (
                    <TestTube2 className="h-4 w-4 mr-2" />
                  )}
                  {t("modelTest.testProvider", "测试模型")}
                </DropdownMenuItem>
              )}
              {onConfigureUsage && (
                <DropdownMenuItem onClick={onConfigureUsage}>
                  <BarChart3 className="h-4 w-4 mr-2" />
                  {t("provider.configureUsage")}
                </DropdownMenuItem>
              )}
              {onOpenTerminal && (
                <DropdownMenuItem onClick={onOpenTerminal}>
                  <Terminal className="h-4 w-4 mr-2" />
                  {t("provider.openTerminal", "打开终端")}
                </DropdownMenuItem>
              )}
              <DropdownMenuSeparator />
              <DropdownMenuItem
                onClick={onDelete}
                className="text-red-600 dark:text-red-400 focus:text-red-600 dark:focus:text-red-400"
              >
                <Trash2 className="h-4 w-4 mr-2" />
                {t("common.delete")}
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
      ) : (
        /* 标准模式：显示所有按钮 */
        <div className="flex items-center gap-2">
          <Button
            size="icon"
            variant="ghost"
            onClick={onEdit}
            title={t("common.edit")}
            className={iconButtonClass}
          >
            <Edit className="h-4 w-4" />
          </Button>

          <Button
            size="icon"
            variant="ghost"
            onClick={onDuplicate}
            title={t("provider.duplicate")}
            className={iconButtonClass}
          >
            <Copy className="h-4 w-4" />
          </Button>

          {onTest && (
            <Button
              size="icon"
              variant="ghost"
              onClick={onTest}
              disabled={isTesting}
              title={t("modelTest.testProvider", "测试模型")}
              className={iconButtonClass}
            >
              {isTesting ? (
                <Loader2 className="h-4 w-4 animate-spin" />
              ) : (
                <TestTube2 className="h-4 w-4" />
              )}
            </Button>
          )}

          {onConfigureUsage && (
            <Button
              size="icon"
              variant="ghost"
              onClick={onConfigureUsage}
              title={t("provider.configureUsage")}
              className={iconButtonClass}
            >
              <BarChart3 className="h-4 w-4" />
            </Button>
          )}

          {onOpenTerminal && (
            <Button
              size="icon"
              variant="ghost"
              onClick={onOpenTerminal}
              title={t("provider.openTerminal", "打开终端")}
              className={cn(
                iconButtonClass,
                "hover:text-emerald-600 dark:hover:text-emerald-400",
              )}
            >
              <Terminal className="h-4 w-4" />
            </Button>
          )}

          <Button
            size="icon"
            variant="ghost"
            onClick={onDelete}
            title={t("common.delete")}
            className={cn(
              iconButtonClass,
              "hover:text-red-500 dark:hover:text-red-400",
            )}
          >
            <Trash2 className="h-4 w-4" />
          </Button>
        </div>
      )}
    </div>
  );
}
