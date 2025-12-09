import {
  BarChart3,
  Check,
  Copy,
  Edit,
  Loader2,
  Play,
  TestTube2,
  Trash2,
  RotateCcw,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

interface ProviderActionsProps {
  isCurrent: boolean;
  isTesting?: boolean;
  onSwitch: () => void;
  onEdit: () => void;
  onDuplicate: () => void;
  onTest?: () => void;
  onConfigureUsage: () => void;
  onDelete: () => void;
  onResetCircuitBreaker?: () => void;
  isProxyTarget?: boolean;
  consecutiveFailures?: number;
}

export function ProviderActions({
  isCurrent,
  isTesting,
  onSwitch,
  onEdit,
  onDuplicate,
  onTest,
  onConfigureUsage,
  onDelete,
  onResetCircuitBreaker,
  isProxyTarget,
  consecutiveFailures = 0,
}: ProviderActionsProps) {
  const { t } = useTranslation();
  const iconButtonClass = "h-8 w-8 p-1";

  return (
    <div className="flex items-center gap-1.5">
      <Button
        size="sm"
        variant={isCurrent ? "secondary" : "default"}
        onClick={onSwitch}
        disabled={isCurrent}
        className={cn(
          "w-[4.5rem] px-2.5",
          isCurrent &&
            "bg-gray-200 text-muted-foreground hover:bg-gray-200 hover:text-muted-foreground dark:bg-gray-700 dark:hover:bg-gray-700",
        )}
      >
        {isCurrent ? (
          <>
            <Check className="h-4 w-4" />
            {t("provider.inUse")}
          </>
        ) : (
          <>
            <Play className="h-4 w-4" />
            {t("provider.enable")}
          </>
        )}
      </Button>

      <div className="flex items-center gap-1">
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

        <Button
          size="icon"
          variant="ghost"
          onClick={onConfigureUsage}
          title={t("provider.configureUsage")}
          className={iconButtonClass}
        >
          <BarChart3 className="h-4 w-4" />
        </Button>

        {/* 重置熔断器按钮 - 代理目标启用时显示 */}
        {onResetCircuitBreaker && isProxyTarget && (
          <Button
            size="icon"
            variant="ghost"
            onClick={onResetCircuitBreaker}
            disabled={consecutiveFailures === 0}
            title={
              consecutiveFailures > 0
                ? t("provider.resetCircuitBreaker", {
                    defaultValue: "重置熔断器",
                  })
                : t("provider.noFailures", {
                    defaultValue: "当前无失败记录",
                  })
            }
            className={cn(
              iconButtonClass,
              consecutiveFailures > 0 &&
                "hover:text-orange-500 dark:hover:text-orange-400",
            )}
          >
            <RotateCcw className="h-4 w-4" />
          </Button>
        )}

        <Button
          size="icon"
          variant="ghost"
          onClick={isCurrent ? undefined : onDelete}
          title={t("common.delete")}
          className={cn(
            iconButtonClass,
            !isCurrent && "hover:text-red-500 dark:hover:text-red-400",
            isCurrent && "opacity-40 cursor-not-allowed text-muted-foreground",
          )}
        >
          <Trash2 className="h-4 w-4" />
        </Button>
      </div>
    </div>
  );
}
