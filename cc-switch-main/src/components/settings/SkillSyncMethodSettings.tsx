import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import type { SkillSyncMethod } from "@/types";

export interface SkillSyncMethodSettingsProps {
  value: SkillSyncMethod;
  onChange: (value: SkillSyncMethod) => void;
}

export function SkillSyncMethodSettings({
  value,
  onChange,
}: SkillSyncMethodSettingsProps) {
  const { t } = useTranslation();

  // Handle default values: undefined or "auto" defaults to symlink display
  const displayValue = value === "copy" ? "copy" : "symlink";

  return (
    <section className="space-y-2">
      <header className="space-y-1">
        <h3 className="text-sm font-medium">{t("settings.skillSync.title")}</h3>
        <p className="text-xs text-muted-foreground">
          {t("settings.skillSync.description")}
        </p>
      </header>
      <div className="inline-flex gap-1 rounded-md border border-border-default bg-background p-1">
        <SyncMethodButton
          active={displayValue === "symlink"}
          onClick={() => onChange("symlink")}
        >
          {t("settings.skillSync.symlink")}
        </SyncMethodButton>
        <SyncMethodButton
          active={displayValue === "copy"}
          onClick={() => onChange("copy")}
        >
          {t("settings.skillSync.copy")}
        </SyncMethodButton>
      </div>
      {displayValue === "symlink" && (
        <p className="text-xs text-muted-foreground">
          {t("settings.skillSync.symlinkHint")}
        </p>
      )}
    </section>
  );
}

interface SyncMethodButtonProps {
  active: boolean;
  onClick: () => void;
  children: React.ReactNode;
}

function SyncMethodButton({
  active,
  onClick,
  children,
}: SyncMethodButtonProps) {
  return (
    <Button
      type="button"
      onClick={onClick}
      size="sm"
      variant={active ? "default" : "ghost"}
      className={cn(
        "min-w-[96px]",
        active
          ? "shadow-sm"
          : "text-muted-foreground hover:text-foreground hover:bg-muted",
      )}
    >
      {children}
    </Button>
  );
}
