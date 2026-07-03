import {
  Activity,
  BarChart3,
  Check,
  Copy,
  Edit,
  Loader2,
  Play,
  Terminal,
  Trash2,
  Zap,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import type { AppId } from "@/lib/api";

interface ProviderActionsProps {
  appId?: AppId;
  isCurrent: boolean;
  isInConfig?: boolean;
  isTesting?: boolean;
  isProxyTakeover?: boolean;
  isOmo?: boolean;
  onSwitch: () => void;
  onEdit: () => void;
  onDuplicate: () => void;
  onTest?: () => void;
  onConfigureUsage?: () => void;
  onDelete: () => void;
  onRemoveFromConfig?: () => void;
  onDisableOmo?: () => void;
  onOpenTerminal?: () => void;
  isOfficialBlockedByProxy?: boolean;
  // Hermes v12+ providers: dict overlay — edit/delete must go through Web UI
  isReadOnly?: boolean;
  // OpenClaw: default model
  isDefaultModel?: boolean;
  onSetAsDefault?: () => void;
}

// 主按钮的呈现状态。title 用于 disabled 态向用户解释为何不可点击；
// 因 Button 基类带 disabled:pointer-events-none，title 必须挂在外层非禁用
// 的 wrapper 上才会在 hover 时显示（见下方 <span> 包裹）。
interface MainButtonState {
  disabled: boolean;
  variant: "default" | "secondary";
  className: string;
  icon: JSX.Element;
  text: string;
  title?: string;
}

export function ProviderActions({
  appId,
  isCurrent,
  isInConfig = false,
  isTesting,
  isProxyTakeover = false,
  isOmo = false,
  onSwitch,
  onEdit,
  onDuplicate,
  onTest,
  onConfigureUsage,
  onDelete,
  onDisableOmo,
  onOpenTerminal,
  isOfficialBlockedByProxy = false,
  isReadOnly = false,
  // OpenClaw: default model
  isDefaultModel = false,
  onSetAsDefault,
}: ProviderActionsProps) {
  const { t } = useTranslation();
  const iconButtonClass = "h-8 w-8 p-1";

  // 累加模式应用（OpenCode 非 OMO / OpenClaw / Hermes）
  const isAdditiveMode =
    (appId === "opencode" && !isOmo) ||
    appId === "openclaw" ||
    appId === "hermes";

  // 注：per-provider 轮换 toggle 不再「替换」主按钮，而是独立的右侧
  // 图标按钮（在 onToggleFailover 存在时显示）。isFailoverMode 这个
  // 「覆盖主按钮」的状态分支已废弃——主按钮现在统一是「启用 / 已在用」，
  // 轮换 toggle 跟编辑、复制、检测、删除那排图标并列。
  //
  // 全局 auto_failover_enabled 跟 per-provider in_failover_queue
  // 是两层独立的开关：全局决定「是否走 provider_router」，per-provider
  // 决定「这个 provider 是否参与轮换」。它们由 forwarder 在 Rust 端
  // 组合判断——UI 这里只暴露独立的 toggle 给用户，不做耦合。
  const handleMainButtonClick = () => {
    if (isOmo) {
      if (isCurrent) {
        onDisableOmo?.();
      } else {
        onSwitch();
      }
    } else {
      onSwitch();
    }
  };

  const getMainButtonState = (): MainButtonState => {
    if (isOmo) {
      if (isCurrent) {
        return {
          disabled: false,
          variant: "secondary" as const,
          className:
            "bg-gray-200 text-muted-foreground hover:bg-gray-200 hover:text-muted-foreground dark:bg-gray-700 dark:hover:bg-gray-700",
          icon: <Check className="h-4 w-4" />,
          text: t("provider.inUse"),
        };
      }
      return {
        disabled: false,
        variant: "default" as const,
        className: "",
        icon: <Play className="h-4 w-4" />,
        text: t("provider.enable"),
      };
    }

    // 累加模式应用（OpenCode 非 OMO / OpenClaw / Hermes）改为走默认分支：
    // 按钮显示「启用 / 已在用」，点击直接 onSwitch 切换当前 provider——
    // 恢复旧版行为。OpenCode 多 provider 共存的逻辑由后端 provider_router
    // 处理：第一个启用的进 active，第二个启用替换第一个；不再用「+ / -」
    // 表示「添加 / 移除」。
    //
    // 故障转移不再挤占主按钮——它走右侧独立的「轮询 toggle 图标按钮」
    // （`onToggleFailover` 分支），主按钮恢复成纯启用/已在用语义。
    if (isCurrent) {
      return {
        disabled: true,
        variant: "secondary" as const,
        className:
          "bg-gray-200 text-muted-foreground hover:bg-gray-200 hover:text-muted-foreground dark:bg-gray-700 dark:hover:bg-gray-700",
        icon: <Check className="h-4 w-4" />,
        text: t("provider.inUse"),
      };
    }

    if (isOfficialBlockedByProxy) {
      return {
        disabled: true,
        variant: "default" as const,
        className: "",
        icon: <Play className="h-4 w-4" />,
        text: t("provider.enable"),
        title: t("provider.blockedByProxyHint"),
      };
    }

    return {
      disabled: false,
      variant: "default" as const,
      className: isProxyTakeover
        ? "bg-emerald-500 hover:bg-emerald-600 dark:bg-emerald-600 dark:hover:bg-emerald-700"
        : "",
      icon: <Play className="h-4 w-4" />,
      text: t("provider.enable"),
    };
  };

  const buttonState = getMainButtonState();

  const canDelete =
    !isReadOnly && (isOmo || isAdditiveMode ? true : !isCurrent);
  const readOnlyHint = t("provider.managedByHermesHint", {
    defaultValue: "由 Hermes 管理，请在 Hermes Web UI 中编辑",
  });

  return (
    <div className="flex items-center gap-1.5">
      {(appId === "openclaw" || appId === "hermes") &&
        isInConfig &&
        onSetAsDefault &&
        (() => {
          const activeLabel =
            appId === "hermes"
              ? t("provider.inUse", { defaultValue: "已在用" })
              : t("provider.isDefault", { defaultValue: "当前默认" });
          const inactiveLabel =
            appId === "hermes"
              ? t("provider.enable", { defaultValue: "启用" })
              : t("provider.setAsDefault", { defaultValue: "设为默认" });
          return (
            <Button
              size="sm"
              variant={isDefaultModel ? "secondary" : "default"}
              onClick={isDefaultModel ? undefined : onSetAsDefault}
              disabled={isDefaultModel}
              className={cn(
                "w-fit px-2.5",
                isDefaultModel
                  ? "bg-gray-200 text-muted-foreground dark:bg-gray-700 opacity-60 cursor-not-allowed"
                  : "bg-blue-500 hover:bg-blue-600 dark:bg-blue-600 dark:hover:bg-blue-700",
              )}
            >
              <Zap className="h-4 w-4" />
              {isDefaultModel ? activeLabel : inactiveLabel}
            </Button>
          );
        })()}

      {/* wrapper span 承接 hover：disabled 按钮自身 pointer-events:none，
          原生 title 与 cursor 都必须挂在未禁用的外层元素上才会生效 */}
      <span
        title={buttonState.title}
        className={cn(
          "inline-flex",
          buttonState.disabled && "cursor-not-allowed",
        )}
      >
        <Button
          size="sm"
          variant={buttonState.variant}
          onClick={handleMainButtonClick}
          disabled={buttonState.disabled}
          className={cn("w-[4.5rem] px-2.5", buttonState.className)}
        >
          {buttonState.icon}
          {buttonState.text}
        </Button>
      </span>

      <div className="flex items-center gap-1">
        <Button
          size="icon"
          variant="ghost"
          onClick={isReadOnly ? undefined : onEdit}
          disabled={isReadOnly}
          title={isReadOnly ? readOnlyHint : t("common.edit")}
          className={cn(
            iconButtonClass,
            isReadOnly && "opacity-40 cursor-not-allowed text-muted-foreground",
          )}
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

        <Button
          size="icon"
          variant="ghost"
          onClick={onTest || undefined}
          disabled={isTesting}
          title={t("provider.connectivityCheck", "检测连通")}
          className={cn(
            iconButtonClass,
            !onTest && "opacity-40 cursor-not-allowed text-muted-foreground",
          )}
        >
          {isTesting ? (
            <Loader2 className="h-4 w-4 animate-spin" />
          ) : (
            <Activity className="h-4 w-4" />
          )}
        </Button>

        <Button
          size="icon"
          variant="ghost"
          onClick={onConfigureUsage || undefined}
          title={t("provider.configureUsage")}
          className={cn(
            iconButtonClass,
            !onConfigureUsage &&
              "opacity-40 cursor-not-allowed text-muted-foreground",
          )}
        >
          <BarChart3 className="h-4 w-4" />
        </Button>

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
          onClick={canDelete ? onDelete : undefined}
          title={isReadOnly ? readOnlyHint : t("common.delete")}
          className={cn(
            iconButtonClass,
            canDelete && "hover:text-red-500 dark:hover:text-red-400",
            !canDelete && "opacity-40 cursor-not-allowed text-muted-foreground",
          )}
        >
          <Trash2 className="h-4 w-4" />
        </Button>
      </div>
    </div>
  );
}
