import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Loader2, RefreshCw } from "lucide-react";
import { toast } from "@/lib/toast";
import { useSettings } from "@/hooks/useSettings";
import {
  useCcSwitchSkillsDir,
  useInstalledSkills,
  useResyncSkillsToApps,
} from "@/hooks/useSkills";
import { DialogTitle } from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import {
  RADIO_CLASS,
  V7ConfirmDialog,
  V7Dialog,
} from "@/components/mcp/formBits";
import { APP_DISPLAY_NAME } from "@/components/shell/AppGlyph";
import { shortenHomePath } from "@/components/sessions/utils";
import { skillsApi, type SkillAppSyncOutcome } from "@/lib/api/skills";
import type { AppId } from "@/lib/api/types";
import type { SkillStorageLocation, SkillSyncMethod } from "@/types";
import { extractErrorMessage } from "@/utils/errorUtils";

interface SkillsStorageSheetProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

const appName = (app: string) => APP_DISPLAY_NAME[app as AppId] ?? app;

const DEFAULT_CC_SWITCH_DIR = "~/.cc-switch/skills";
const UNIFIED_DIR = "~/.agents/skills";

/** 和后端 SyncMethod（auto / symlink / copy）一一对应。 */
const SYNC_METHODS: {
  id: SkillSyncMethod;
  labelKey: string;
  descKey?: string;
}[] = [
  { id: "auto", labelKey: "skills.storageSheet.syncAuto" },
  {
    id: "symlink",
    labelKey: "skills.storageSheet.syncSymlink",
    descKey: "skills.storageSheet.syncSymlinkDesc",
  },
  {
    id: "copy",
    labelKey: "skills.storageSheet.syncCopy",
    descKey: "skills.storageSheet.syncCopyDesc",
  },
];

const LEGEND_CLASS = "mb-2 p-0 text-body font-semibold text-fg-1";
const RADIO_LABEL_CLASS =
  "flex cursor-pointer items-center gap-2.5 text-body text-fg-1 has-[:disabled]:cursor-not-allowed has-[:disabled]:opacity-60";

/** Skills 页 ⋯ →「存储与同步」对话框：主副本放在哪、怎么同步到各应用（原来在设置 → 通用）。 */
export function SkillsStorageSheet({
  open,
  onOpenChange,
}: SkillsStorageSheetProps) {
  const { t } = useTranslation();
  const { settings, updateSettings, autoSaveSettings } = useSettings();
  const { data: installedSkills } = useInstalledSkills();
  const { data: ccSwitchDirRaw } = useCcSwitchSkillsDir(open);
  const resyncMutation = useResyncSkillsToApps();

  const location: SkillStorageLocation =
    settings?.skillStorageLocation ?? "cc_switch";
  const syncMethod: SkillSyncMethod = settings?.skillSyncMethod ?? "auto";
  const installedCount = installedSkills?.length ?? 0;
  const ccSwitchDir = ccSwitchDirRaw
    ? shortenHomePath(ccSwitchDirRaw)
    : DEFAULT_CC_SWITCH_DIR;

  // 单选只是「想换到哪」；点「移动并切换…」才真正迁移
  const [pendingLocation, setPendingLocation] =
    useState<SkillStorageLocation>(location);
  const [confirmMove, setConfirmMove] = useState(false);
  const [isMigrating, setIsMigrating] = useState(false);

  useEffect(() => {
    if (open) setPendingLocation(location);
  }, [open, location]);

  const targetDir = pendingLocation === "unified" ? UNIFIED_DIR : ccSwitchDir;
  // 对话框里的禁用原因写成看得见的一行（HELP_BRIEF：菜单 / 对话框例外），不挂悬停气泡
  const locationUnchanged = pendingLocation === location;

  const doMigrate = async (target: SkillStorageLocation) => {
    setIsMigrating(true);
    setConfirmMove(false);
    const path = target === "unified" ? UNIFIED_DIR : ccSwitchDir;
    try {
      const result = await skillsApi.migrateStorage(target);
      if (result.errors.length > 0) {
        toast.warning(
          t("skills.storageSheet.movePartial", {
            migrated: result.migratedCount,
            errors: result.errors.length,
          }),
          { description: result.errors.join("\n"), closeButton: true },
        );
      } else {
        toast.success(
          t("skills.storageSheet.moveDone", {
            count: result.migratedCount,
            path,
          }),
          { closeButton: true },
        );
      }
      updateSettings({ skillStorageLocation: target });
    } catch (error) {
      toast.error(t("common.error"), {
        description: extractErrorMessage(error) || String(error),
      });
    } finally {
      setIsMigrating(false);
    }
  };

  const handleMove = () => {
    if (pendingLocation === location || isMigrating) return;
    // 一个 Skill 都没有时没什么可移动的，直接切换
    if (installedCount > 0) {
      setConfirmMove(true);
    } else {
      void doMigrate(pendingLocation);
    }
  };

  const handleOpenFolder = async () => {
    try {
      await skillsApi.openCcSwitchSkillsDir();
    } catch (error) {
      toast.error(t("common.error"), {
        description: extractErrorMessage(error) || String(error),
      });
    }
  };

  const handleSyncMethod = (method: SkillSyncMethod) => {
    if (method === syncMethod) return;
    updateSettings({ skillSyncMethod: method });
    void autoSaveSettings({ skillSyncMethod: method }).catch(() => undefined);
  };

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
    <>
      <V7Dialog
        open={open}
        onOpenChange={(next) => {
          if (!next && isMigrating) return;
          onOpenChange(next);
        }}
        width={520}
      >
        <DialogTitle>{t("skills.storageSheet.title")}</DialogTitle>

        {settings && (
          <>
            <fieldset className="m-0 flex min-w-0 flex-col gap-2.5 border-0 p-0">
              <legend className={LEGEND_CLASS}>
                {t("skills.storageSheet.locationTitle")}
              </legend>
              <div className="flex flex-col gap-0.5">
                <label className={RADIO_LABEL_CLASS}>
                  <input
                    type="radio"
                    name="skills-storage-location"
                    className={RADIO_CLASS}
                    checked={pendingLocation === "cc_switch"}
                    disabled={isMigrating}
                    aria-describedby="skills-storage-cc-path"
                    onChange={() => setPendingLocation("cc_switch")}
                  />
                  {t("skills.storageSheet.locationCcSwitch")}
                </label>
                <div className="flex min-w-0 items-center gap-2 pl-[26px]">
                  <code
                    id="skills-storage-cc-path"
                    className="min-w-0 truncate font-mono text-caption text-fg-2"
                    title={ccSwitchDirRaw ?? ccSwitchDir}
                  >
                    {ccSwitchDir}
                  </code>
                  <button
                    type="button"
                    aria-label={t("skills.storageSheet.openFolderAria", {
                      path: ccSwitchDir,
                    })}
                    onClick={() => void handleOpenFolder()}
                    className="shrink-0 whitespace-nowrap text-caption font-medium text-fg-1 underline decoration-border-strong underline-offset-[3px] hover:decoration-fg-1"
                  >
                    {t("skills.storageSheet.openFolder")}
                  </button>
                </div>
              </div>
              <div className="flex flex-col gap-0.5">
                <label className={RADIO_LABEL_CLASS}>
                  <input
                    type="radio"
                    name="skills-storage-location"
                    className={RADIO_CLASS}
                    checked={pendingLocation === "unified"}
                    disabled={isMigrating}
                    aria-describedby="skills-storage-unified-warn"
                    onChange={() => setPendingLocation("unified")}
                  />
                  <span>
                    {t("skills.storageSheet.locationUnified")}{" "}
                    <code className="whitespace-nowrap font-mono text-caption">
                      {UNIFIED_DIR}
                    </code>
                  </span>
                </label>
                <p
                  id="skills-storage-unified-warn"
                  className="m-0 pl-[26px] text-caption text-warning-text"
                >
                  {t("skills.storageSheet.unifiedWarning")}
                </p>
              </div>
              <div className="flex items-center justify-between gap-3 pt-0.5">
                <p
                  id="skills-storage-move-hint"
                  className="m-0 min-w-0 text-caption text-fg-2"
                >
                  {t("skills.storageSheet.moveHint", { count: installedCount })}
                </p>
                <Button
                  type="button"
                  variant="neutral"
                  size="regular"
                  className="shrink-0"
                  disabled={isMigrating || locationUnchanged}
                  aria-describedby="skills-storage-move-hint"
                  onClick={handleMove}
                >
                  {isMigrating && <Loader2 className="h-4 w-4 animate-spin" />}
                  {isMigrating
                    ? t("skills.storageSheet.moving")
                    : t("skills.storageSheet.move")}
                </Button>
              </div>
            </fieldset>

            <div className="h-px bg-border" />

            <fieldset className="m-0 flex min-w-0 flex-col gap-2 border-0 p-0">
              <legend className={LEGEND_CLASS}>
                {t("skills.storageSheet.syncTitle")}
              </legend>
              {SYNC_METHODS.map((option) => (
                <label key={option.id} className={RADIO_LABEL_CLASS}>
                  <input
                    type="radio"
                    name="skills-sync-method"
                    value={option.id}
                    className={RADIO_CLASS}
                    checked={syncMethod === option.id}
                    onChange={() => handleSyncMethod(option.id)}
                  />
                  <span>
                    {t(option.labelKey)}
                    {option.descKey && (
                      <span className="ml-2 text-caption text-fg-2">
                        {t(option.descKey)}
                      </span>
                    )}
                  </span>
                </label>
              ))}
              <div className="flex items-center gap-3 pt-0.5">
                <p className="m-0 min-w-0 flex-1 text-caption text-fg-2">
                  {t("skills.storageSheet.resyncHint")}
                </p>
                <Button
                  type="button"
                  variant="neutral"
                  size="compact"
                  className="shrink-0"
                  disabled={resyncMutation.isPending}
                  onClick={() => void handleResync()}
                >
                  {resyncMutation.isPending ? (
                    <Loader2 className="h-3.5 w-3.5 animate-spin" />
                  ) : (
                    <RefreshCw className="h-3.5 w-3.5" strokeWidth={2} />
                  )}
                  {resyncMutation.isPending
                    ? t("skills.storageSheet.resyncing")
                    : t("skills.storageSheet.resync")}
                </Button>
              </div>
            </fieldset>
          </>
        )}

        <div className="flex justify-end pt-1">
          <Button
            type="button"
            variant="solid"
            size="regular"
            autoFocus
            disabled={isMigrating}
            onClick={() => onOpenChange(false)}
          >
            {t("common.done")}
          </Button>
        </div>
      </V7Dialog>

      <V7ConfirmDialog
        open={confirmMove}
        danger={false}
        title={t("skills.storageSheet.moveConfirmTitle", {
          count: installedCount,
          path: targetDir,
        })}
        body={t("skills.storageSheet.moveConfirmBody")}
        confirmLabel={t("skills.storageSheet.moveConfirmButton")}
        pending={isMigrating}
        onConfirm={() => void doMigrate(pendingLocation)}
        onCancel={() => setConfirmMove(false)}
      />
    </>
  );
}
