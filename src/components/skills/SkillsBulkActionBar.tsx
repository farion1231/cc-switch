import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Trash2, RefreshCw, X, ChevronDown, Check } from "lucide-react";
import { Button } from "@/components/ui/button";
import { APP_ICON_MAP } from "@/config/appConfig";
import type { AppId } from "@/lib/api/types";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";

interface SkillsBulkActionBarProps {
  selectedCount: number;
  selectedHasUpdate: number;
  appIds: AppId[];
  onUninstall: () => void;
  onToggleApp: (app: AppId, enabled: boolean) => void;
  onUpdateAvailable: () => void;
  onCancel: () => void;
  onSelectAll: () => void;
  totalVisible: number;
  isWorking: boolean;
}

export function SkillsBulkActionBar({
  selectedCount,
  selectedHasUpdate,
  appIds,
  onUninstall,
  onToggleApp,
  onUpdateAvailable,
  onCancel,
  onSelectAll,
  totalVisible,
  isWorking,
}: SkillsBulkActionBarProps) {
  const { t } = useTranslation();
  const [enableOpen, setEnableOpen] = useState(false);
  const [disableOpen, setDisableOpen] = useState(false);

  const allSelected = selectedCount > 0 && selectedCount === totalVisible;

  return (
    <div className="-mx-6 px-6 py-2 border-t border-border-default flex flex-wrap items-center gap-2 bg-muted/30">
      <span className="text-sm font-medium">
        {t("skills.bulk.selected", { count: selectedCount })}
      </span>

      <Button
        type="button"
        variant="link"
        onClick={onSelectAll}
        className="h-auto p-0 text-xs text-muted-foreground hover:text-foreground"
      >
        {allSelected
          ? t("skills.bulk.selectNone")
          : t("skills.bulk.selectAll", { count: totalVisible })}
      </Button>

      <div className="ml-auto flex flex-wrap items-center gap-1.5">
        <DropdownMenu open={enableOpen} onOpenChange={setEnableOpen}>
          <DropdownMenuTrigger asChild>
            <Button
              type="button"
              variant="outline"
              size="sm"
              disabled={selectedCount === 0 || isWorking}
              className="h-8 gap-1"
            >
              <Check className="h-3.5 w-3.5" />
              {t("skills.bulk.enableIn")}
              <ChevronDown className="h-3 w-3" />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end" className="w-44">
            {appIds.map((app) => (
              <DropdownMenuItem
                key={app}
                onSelect={() => onToggleApp(app, true)}
              >
                {APP_ICON_MAP[app]?.label ?? app}
              </DropdownMenuItem>
            ))}
          </DropdownMenuContent>
        </DropdownMenu>

        <DropdownMenu open={disableOpen} onOpenChange={setDisableOpen}>
          <DropdownMenuTrigger asChild>
            <Button
              type="button"
              variant="outline"
              size="sm"
              disabled={selectedCount === 0 || isWorking}
              className="h-8 gap-1"
            >
              <X className="h-3.5 w-3.5" />
              {t("skills.bulk.disableIn")}
              <ChevronDown className="h-3 w-3" />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end" className="w-44">
            {appIds.map((app) => (
              <DropdownMenuItem
                key={app}
                onSelect={() => onToggleApp(app, false)}
              >
                {APP_ICON_MAP[app]?.label ?? app}
              </DropdownMenuItem>
            ))}
          </DropdownMenuContent>
        </DropdownMenu>

        {selectedHasUpdate > 0 && (
          <Button
            type="button"
            variant="outline"
            size="sm"
            disabled={isWorking}
            className="h-8 gap-1"
            onClick={onUpdateAvailable}
          >
            <RefreshCw className="h-3.5 w-3.5" />
            {t("skills.bulk.updateAvailable", { count: selectedHasUpdate })}
          </Button>
        )}

        <Button
          type="button"
          variant="destructive"
          size="sm"
          disabled={selectedCount === 0 || isWorking}
          className="h-8 gap-1"
          onClick={onUninstall}
        >
          <Trash2 className="h-3.5 w-3.5" />
          {t("skills.bulk.uninstall")}
        </Button>

        <Button
          type="button"
          variant="ghost"
          size="sm"
          className="h-8"
          onClick={onCancel}
        >
          {t("skills.bulk.cancel")}
        </Button>
      </div>
    </div>
  );
}
