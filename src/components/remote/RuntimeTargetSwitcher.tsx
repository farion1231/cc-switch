import {
  Check,
  ChevronsUpDown,
  LoaderCircle,
  Monitor,
  Server,
  Settings2,
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
import { useRuntimeTarget } from "@/contexts/RuntimeTargetContext";
import { cn } from "@/lib/utils";

export function RuntimeTargetSwitcher({ onManage }: { onManage: () => void }) {
  const { t } = useTranslation();
  const { snapshot, targets, setActiveTarget } = useRuntimeTarget();
  const activeTarget = targets.find(
    (target) => target.id === snapshot.activeTargetId,
  );
  const isTransitioning =
    snapshot.status === "connecting" || snapshot.status === "reconnecting";
  const label =
    activeTarget?.name ??
    t("remote.local", {
      defaultValue: "本机",
    });

  const dotClass = {
    local: "bg-zinc-400",
    connecting: "bg-amber-500",
    online: "bg-emerald-500",
    reconnecting: "bg-amber-500",
    offline: "bg-red-500",
    incompatible: "bg-red-500",
  }[snapshot.status];

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          variant="ghost"
          size="sm"
          disabled={isTransitioning}
          className="h-8 min-w-0 max-w-44 gap-1.5 px-2.5 text-muted-foreground"
          aria-label={`${label} ${snapshot.status}`}
          title={snapshot.errorMessage || label}
        >
          {isTransitioning ? (
            <LoaderCircle className="h-4 w-4 shrink-0 animate-spin" />
          ) : activeTarget ? (
            <Server className="h-4 w-4 shrink-0" />
          ) : (
            <Monitor className="h-4 w-4 shrink-0" />
          )}
          <span className="truncate">{label}</span>
          <span
            data-testid="runtime-status-dot"
            className={cn("h-2 w-2 shrink-0 rounded-full", dotClass)}
          />
          <ChevronsUpDown className="h-3.5 w-3.5 shrink-0 opacity-50" />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-56">
        <DropdownMenuItem onSelect={() => void setActiveTarget(undefined)}>
          <Monitor className="h-4 w-4" />
          <span className="flex-1">
            {t("remote.local", { defaultValue: "本机" })}
          </span>
          {!snapshot.activeTargetId && <Check className="h-4 w-4" />}
        </DropdownMenuItem>
        {targets.map((target) => (
          <DropdownMenuItem
            key={target.id}
            onSelect={() => void setActiveTarget(target.id)}
          >
            <Server className="h-4 w-4" />
            <span className="min-w-0 flex-1 truncate">{target.name}</span>
            {snapshot.activeTargetId === target.id && (
              <Check className="h-4 w-4" />
            )}
          </DropdownMenuItem>
        ))}
        <DropdownMenuSeparator />
        <DropdownMenuItem onSelect={onManage}>
          <Settings2 className="h-4 w-4" />
          {t("remote.manage", { defaultValue: "管理服务器" })}
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
