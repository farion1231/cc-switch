import { AlertCircle, MonitorCog, Settings2 } from "lucide-react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import type { ManagedTarget } from "@/types";

interface ManagedTargetSelectorProps {
  targets: ManagedTarget[];
  selectedTargetId: string | null;
  onSelect: (targetId: string) => void;
  onManage: () => void;
  isLoading?: boolean;
  error?: string;
}

export function ManagedTargetSelector({
  targets,
  selectedTargetId,
  onSelect,
  onManage,
  isLoading = false,
  error,
}: ManagedTargetSelectorProps) {
  const { t } = useTranslation();

  if (error) {
    return (
      <div className="mb-4 flex items-center justify-between gap-3 rounded-lg border border-red-500/30 bg-red-500/5 p-3 text-sm text-red-600">
        <div className="flex min-w-0 items-center gap-2">
          <AlertCircle className="h-4 w-4 shrink-0" />
          <span className="truncate">
            {t("settings.environments.loadFailed")}: {error}
          </span>
        </div>
        <Button
          type="button"
          variant="outline"
          size="sm"
          className="shrink-0"
          onClick={onManage}
        >
          {t("settings.environments.manage")}
        </Button>
      </div>
    );
  }

  return (
    <div className="mb-4 flex flex-col gap-2 rounded-lg border bg-muted/30 p-3 sm:flex-row sm:items-center sm:justify-between">
      <div className="flex min-w-0 items-center gap-3">
        <MonitorCog className="h-5 w-5 shrink-0 text-muted-foreground" />
        <div className="min-w-0">
          <p className="text-sm font-medium">
            {t("settings.environments.activeTarget")}
          </p>
          <p className="text-xs text-muted-foreground">
            {t("settings.environments.activeTargetNotice")}
          </p>
        </div>
      </div>
      <div className="flex items-center gap-2">
        <Select
          value={selectedTargetId ?? undefined}
          onValueChange={onSelect}
          disabled={isLoading || targets.length === 0}
        >
          <SelectTrigger className="w-full min-w-52 sm:w-72">
            <SelectValue
              placeholder={t("settings.environments.selectTarget")}
            />
          </SelectTrigger>
          <SelectContent>
            {targets.map((target) => (
              <SelectItem key={target.id} value={target.id}>
                {target.name}
                {target.kind.type === "wsl"
                  ? ` · WSL ${target.kind.distro}`
                  : " · Windows"}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <Button
          type="button"
          variant="outline"
          size="icon"
          onClick={onManage}
          title={t("settings.environments.manage")}
        >
          <Settings2 className="h-4 w-4" />
          <span className="sr-only">{t("settings.environments.manage")}</span>
        </Button>
      </div>
    </div>
  );
}
