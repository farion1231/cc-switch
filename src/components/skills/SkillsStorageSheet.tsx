import { useTranslation } from "react-i18next";
import { useSettings } from "@/hooks/useSettings";
import { useInstalledSkills } from "@/hooks/useSkills";
import {
  Sheet,
  SheetBody,
  SheetContent,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet";
import { SkillStorageLocationSettings } from "@/components/settings/SkillStorageLocationSettings";
import { SkillSyncMethodSettings } from "@/components/settings/SkillSyncMethodSettings";

interface SkillsStorageSheetProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

/** Skills 页 →「存储与同步」：存在哪、怎么同步到各应用（原来在设置 → 通用）。 */
export function SkillsStorageSheet({
  open,
  onOpenChange,
}: SkillsStorageSheetProps) {
  const { t } = useTranslation();
  const { settings, updateSettings, autoSaveSettings } = useSettings();
  const { data: installedSkills } = useInstalledSkills();

  return (
    <Sheet open={open} onOpenChange={onOpenChange}>
      <SheetContent width={480} closeLabel={t("common.close")}>
        <SheetHeader>
          <SheetTitle>{t("skills.storageSheet.title")}</SheetTitle>
        </SheetHeader>
        <SheetBody className="space-y-6">
          {settings && (
            <>
              <SkillStorageLocationSettings
                value={settings.skillStorageLocation ?? "cc_switch"}
                installedCount={installedSkills?.length ?? 0}
                onMigrated={(location) =>
                  updateSettings({ skillStorageLocation: location })
                }
              />
              <SkillSyncMethodSettings
                value={settings.skillSyncMethod ?? "auto"}
                onChange={(method) => {
                  updateSettings({ skillSyncMethod: method });
                  void autoSaveSettings({ skillSyncMethod: method }).catch(
                    () => undefined,
                  );
                }}
              />
            </>
          )}
        </SheetBody>
      </SheetContent>
    </Sheet>
  );
}
