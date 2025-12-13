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
import type { LaunchConfigSet } from "@/hooks/useConfigSets";

interface ProviderActionsProps {
  isCurrent: boolean;
  onSwitch: (configSetId?: string) => void | Promise<void>;
  isTesting?: boolean;
  isProxyTakeover?: boolean;
  onEdit: () => void;
  onDuplicate: () => void;
  onTest?: () => void;
  onConfigureUsage: () => void;
  onDelete: () => void;
  configSets?: LaunchConfigSet[];
  activeConfigSetId?: string;
  isSwitching?: boolean;
  onResetCircuitBreaker?: () => void;
  isProxyTarget?: boolean;
  consecutiveFailures?: number;
}

export function ProviderActions({
  isCurrent,
  isTesting,
  isProxyTakeover = false,
  onSwitch,
  onEdit,
  onDuplicate,
  onTest,
  onConfigureUsage,
  onDelete,
  configSets,
  activeConfigSetId,
  isSwitching = false,
  onResetCircuitBreaker,
  isProxyTarget,
  consecutiveFailures = 0,
}: ProviderActionsProps) {
  const { t } = useTranslation();
  const iconButtonClass = "h-8 w-8 p-1";
  const defaultConfigSetId =
    activeConfigSetId ?? configSets?.[0]?.id ?? undefined;

  const handleSwitch = (configSetId?: string) => {
    const target = configSetId ?? defaultConfigSetId;
    void onSwitch(target);
  };

  const renderContent = (label: "inUse" | "enable") => (
    <>
      {isSwitching ? (
        <Loader2 className="h-4 w-4 animate-spin" />
      ) : label === "inUse" ? (
        <Check className="h-4 w-4" />
      ) : (
        <Play className="h-4 w-4" />
      )}
      {label === "inUse" ? t("provider.inUse") : t("provider.enable")}
    </>
  );

  const handleEnableClick = () => {
    if (isCurrent || isSwitching) return;
    handleSwitch();
  };

  const renderEnableButton = () => (
    <Button
      size="sm"
      variant={isCurrent ? "secondary" : "default"}
      disabled={isCurrent || isSwitching}
      onClick={handleEnableClick}
      className={cn(
        "min-w-[4.5rem] px-2.5",
        isCurrent &&
          "bg-gray-200 text-muted-foreground hover:bg-gray-200 hover:text-muted-foreground dark:bg-gray-700 dark:hover:bg-gray-700",
        !isCurrent &&
          isProxyTakeover &&
          "bg-emerald-500 hover:bg-emerald-600 dark:bg-emerald-600 dark:hover:bg-emerald-700",
      )}
    >
      {renderContent(isCurrent ? "inUse" : "enable")}
    </Button>
  );

  return (
    <div className="flex items-center gap-1.5">
      {renderEnableButton()}

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
            isCurrent && "opacity-40 cursor-not-allowed text-muted-foreground"
          )}
        >
          <Trash2 className="h-4 w-4" />
        </Button>
      </div>
    </div>
  );
}
