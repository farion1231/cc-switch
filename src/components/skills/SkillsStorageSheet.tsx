import { useTranslation } from "react-i18next";
import { Loader2, RefreshCw } from "lucide-react";
import { toast } from "sonner";
import { useSettings } from "@/hooks/useSettings";
import { useInstalledSkills, useResyncSkillsToApps } from "@/hooks/useSkills";
import {
  Sheet,
  SheetBody,
  SheetContent,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet";
import { Button } from "@/components/ui/button";
import { APP_DISPLAY_NAME } from "@/components/shell/AppGlyph";
import { SkillStorageLocationSettings } from "@/components/settings/SkillStorageLocationSettings";
import { SkillSyncMethodSettings } from "@/components/settings/SkillSyncMethodSettings";
import type { SkillAppSyncOutcome } from "@/lib/api/skills";
import type { AppId } from "@/lib/api/types";
import { extractErrorMessage } from "@/utils/errorUtils";

interface SkillsStorageSheetProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

const appName = (app: string) => APP_DISPLAY_NAME[app as AppId] ?? app;

/** Skills 页 →「存储与同步」：存在哪、怎么同步到各应用（原来在设置 → 通用）。 */
export function SkillsStorageSheet({
  open,
  onOpenChange,
}: SkillsStorageSheetProps) {
  const { t } = useTranslation();
  const { settings, updateSettings, autoSaveSettings } = useSettings();
  const { data: installedSkills } = useInstalledSkills();
  const resyncMutation = useResyncSkillsToApps();

  const reportResync = (outcomes: SkillAppSyncOutcome[]) => {
    const failed = outcomes.filter((outcome) => !outcome.ok);
    if (failed.length === 0) {
      toast.success(t("skills.storageSheet.resyncDone"), { closeButton: true });
      return;
    }
    const details = failed.flatMap((outcome) =>
      outcome.error
        ? [`${appName(outcome.app)}: ${outcome.error}`]
        : outcome.failedSkills.map(
            (skill) =>
              `${appName(outcome.app)} · ${t(
                "skills.storageSheet.resyncSkillItem",
                { directory: skill.directory, error: skill.error },
              )}`,
          ),
    );
    toast.warning(
      t("skills.storageSheet.resyncPartial", {
        count: failed.length,
        apps: failed
          .map((outcome) => appName(outcome.app))
          .join(t("mcpPage.listSeparator")),
      }),
      { description: details.join("\n"), closeButton: true },
    );
  };

  const handleResync = async () => {
    if (resyncMutation.isPending) return;
    try {
      reportResync(await resyncMutation.mutateAsync());
    } catch (error) {
      toast.error(t("common.error"), {
        description: extractErrorMessage(error) || String(error),
      });
    }
  };

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
              <div className="space-y-3">
                <SkillSyncMethodSettings
                  value={settings.skillSyncMethod ?? "auto"}
                  onChange={(method) => {
                    updateSettings({ skillSyncMethod: method });
                    void autoSaveSettings({ skillSyncMethod: method }).catch(
                      () => undefined,
                    );
                  }}
                />
                <p className="m-0 text-caption text-fg-2">
                  {t("skills.storageSheet.resyncHint")}
                </p>
                <Button
                  type="button"
                  variant="neutral"
                  size="regular"
                  disabled={resyncMutation.isPending}
                  onClick={() => void handleResync()}
                >
                  {resyncMutation.isPending ? (
                    <Loader2 className="h-4 w-4 animate-spin" />
                  ) : (
                    <RefreshCw className="h-4 w-4" strokeWidth={2} />
                  )}
                  {resyncMutation.isPending
                    ? t("skills.storageSheet.resyncing")
                    : t("skills.storageSheet.resync")}
                </Button>
              </div>
            </>
          )}
        </SheetBody>
      </SheetContent>
    </Sheet>
  );
}
