import { BarChart3, Check, Copy, Edit, Loader2, Play, Trash2 } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import type { LaunchConfigSet } from "@/hooks/useConfigSets";

interface ProviderActionsProps {
  isCurrent: boolean;
  onSwitch: (configSetId?: string) => void | Promise<void>;
  onEdit: () => void;
  onDuplicate: () => void;
  onConfigureUsage: () => void;
  onDelete: () => void;
  configSets?: LaunchConfigSet[];
  activeConfigSetId?: string;
  isSwitching?: boolean;
}

export function ProviderActions({
  isCurrent,
  onSwitch,
  onEdit,
  onDuplicate,
  onConfigureUsage,
  onDelete,
  configSets,
  activeConfigSetId,
  isSwitching = false,
}: ProviderActionsProps) {
  const { t } = useTranslation();
  const iconButtonClass = "h-8 w-8 p-1";
  const defaultConfigSetId =
    activeConfigSetId ?? configSets?.[0]?.id ?? undefined;

  const handleSwitch = (configSetId?: string) => {
    const target = configSetId ?? defaultConfigSetId;
    void onSwitch(target);
  };

  const renderContent = (label: string) => (
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

  const renderEnableButton = () => {
    if (isCurrent) {
      return (
        <Button
          size="sm"
          variant="secondary"
          disabled
          className="min-w-[4.5rem] px-2.5 bg-gray-200 text-muted-foreground hover:bg-gray-200 hover:text-muted-foreground dark:bg-gray-700 dark:hover:bg-gray-700"
        >
          {renderContent("inUse")}
        </Button>
      );
    }

    return (
      <Button
        size="sm"
        variant="default"
        className="min-w-[4.5rem] px-2.5"
        disabled={isSwitching}
        onClick={() => handleSwitch()}
      >
        {renderContent("enable")}
      </Button>
    );
  };

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

        <Button
          size="icon"
          variant="ghost"
          onClick={onConfigureUsage}
          title={t("provider.configureUsage")}
          className={iconButtonClass}
        >
          <BarChart3 className="h-4 w-4" />
        </Button>

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
