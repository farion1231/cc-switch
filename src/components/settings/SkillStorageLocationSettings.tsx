import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Loader2 } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { cn } from "@/lib/utils";
import { skillsApi, type MigrationResult } from "@/lib/api/skills";
import { useSkillStorageDir } from "@/hooks/useSkills";
import type { SkillStorageLocation } from "@/types";

export interface SkillStorageLocationSettingsProps {
  value: SkillStorageLocation;
  installedCount: number;
  onMigrated: (target: SkillStorageLocation) => void;
}

export function SkillStorageLocationSettings({
  value,
  installedCount,
  onMigrated,
}: SkillStorageLocationSettingsProps) {
  const { t } = useTranslation();
  const { data: storageDir } = useSkillStorageDir(value);
  const [pendingTarget, setPendingTarget] =
    useState<SkillStorageLocation | null>(null);
  const [isMigrating, setIsMigrating] = useState(false);

  const handleSelect = (target: SkillStorageLocation) => {
    if (target === value) return;
    if (installedCount > 0) {
      setPendingTarget(target);
    } else {
      doMigrate(target);
    }
  };

  const doMigrate = async (target: SkillStorageLocation) => {
    setIsMigrating(true);
    setPendingTarget(null);
    try {
      const result: MigrationResult = await skillsApi.migrateStorage(target);
      if (result.errors.length > 0) {
        toast.warning(
          t("settings.skillStorage.migrationPartial", {
            migrated: result.migratedCount,
            errors: result.errors.length,
          }),
        );
      } else {
        toast.success(
          t("settings.skillStorage.migrationSuccess", {
            count: result.migratedCount,
          }),
        );
      }
      onMigrated(target);
    } catch (error) {
      toast.error(String(error));
    } finally {
      setIsMigrating(false);
    }
  };

  return (
    <section className="space-y-2">
      <header className="space-y-1">
        <h3 className="text-sm font-medium">
          {t("settings.skillStorage.title")}
        </h3>
        <p className="text-xs text-muted-foreground">
          {t("settings.skillStorage.description")}
        </p>
      </header>
      <div className="inline-flex gap-1 rounded-md border border-border-default bg-background p-1">
        <StorageButton
          active={value === "cc_switch"}
          disabled={isMigrating}
          onClick={() => handleSelect("cc_switch")}
        >
          {t("settings.skillStorage.ccSwitch")}
        </StorageButton>
        <StorageButton
          active={value === "unified"}
          disabled={isMigrating}
          onClick={() => handleSelect("unified")}
        >
          {isMigrating && value !== "unified" ? (
            <Loader2 size={14} className="mr-1 animate-spin" />
          ) : null}
          {t("settings.skillStorage.unified")}
        </StorageButton>
      </div>
      <p className="text-xs text-muted-foreground">
        {value === "unified"
          ? t("settings.skillStorage.unifiedHint")
          : t("settings.skillStorage.ccSwitchHint", {
              // 后端解析的真实路径（遵循配置目录覆盖）；加载完成前先按默认位置展示
              path: storageDir ?? "~/.cc-switch/skills",
            })}
      </p>

      {/* 迁移确认对话框 */}
      <Dialog
        open={pendingTarget !== null}
        onOpenChange={(open) => {
          if (!open) setPendingTarget(null);
        }}
      >
        <DialogContent className="max-w-md" zIndex="alert">
          <DialogHeader>
            <DialogTitle>{t("settings.skillStorage.confirmTitle")}</DialogTitle>
            <DialogDescription>
              {t("settings.skillStorage.confirmMessage", {
                count: installedCount,
              })}
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setPendingTarget(null)}>
              {t("common.cancel")}
            </Button>
            <Button onClick={() => pendingTarget && doMigrate(pendingTarget)}>
              {t("common.confirm")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </section>
  );
}

interface StorageButtonProps {
  active: boolean;
  disabled?: boolean;
  onClick: () => void;
  children: React.ReactNode;
}

function StorageButton({
  active,
  disabled,
  onClick,
  children,
}: StorageButtonProps) {
  return (
    <Button
      type="button"
      onClick={onClick}
      disabled={disabled}
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
