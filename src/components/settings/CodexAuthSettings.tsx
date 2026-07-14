import { useState } from "react";
import { useTranslation } from "react-i18next";
import { History, KeyRound, Loader2, Wrench } from "lucide-react";
import { toast } from "sonner";
import type { SettingsFormState } from "@/hooks/useSettings";
import { ToggleRow } from "@/components/ui/toggle-row";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { Button } from "@/components/ui/button";
import { settingsApi, type CodexHistoryVisibilityDiagnosis } from "@/lib/api";

interface CodexAuthSettingsProps {
  settings: SettingsFormState;
  /** 返回 false（或 resolve 为 false）表示保存失败；其余返回值视为成功 */
  onChange: (
    updates: Partial<SettingsFormState>,
  ) => void | boolean | Promise<void | boolean>;
}

export function CodexAuthSettings({
  settings,
  onChange,
}: CodexAuthSettingsProps) {
  const { t } = useTranslation();
  const [showEnableConfirm, setShowEnableConfirm] = useState(false);
  const [showDisableConfirm, setShowDisableConfirm] = useState(false);
  const [showRepairConfirm, setShowRepairConfirm] = useState(false);
  const [hasUnifyBackup, setHasUnifyBackup] = useState(false);
  const [historyDiagnosis, setHistoryDiagnosis] =
    useState<CodexHistoryVisibilityDiagnosis | null>(null);
  const [isDiagnosingHistory, setIsDiagnosingHistory] = useState(false);
  const [isRepairingHistory, setIsRepairingHistory] = useState(false);

  const handleUnifyHistoryChange = (checked: boolean) => {
    if (checked) {
      setShowEnableConfirm(true);
      return;
    }
    // 先探测有无迁移备份，决定关闭弹窗是否提供"恢复备份"勾选
    void settingsApi
      .hasCodexUnifyHistoryBackup()
      .catch(() => false)
      .then((hasBackup) => {
        setHasUnifyBackup(hasBackup);
        setShowDisableConfirm(true);
      });
  };

  const handleEnableConfirm = (migrateExisting: boolean) => {
    setShowEnableConfirm(false);
    void onChange({
      unifyCodexSessionHistory: true,
      unifyCodexMigrateExisting: migrateExisting,
    });
  };

  // 备份探测可能落后于正在后台进行的迁移（刚勾选迁入就立刻关闭时，
  // 备份尚未产出）。只要本轮勾选过"迁入既有会话"，就必须提供恢复入口；
  // 真正有没有账本交给后端 restore 的 skippedReason 判定。
  const showRestoreOption =
    hasUnifyBackup || (settings.unifyCodexMigrateExisting ?? false);

  const handleDisableConfirm = async (restoreBackup: boolean) => {
    setShowDisableConfirm(false);
    const saved = await onChange({
      unifyCodexSessionHistory: false,
      unifyCodexMigrateExisting: false,
    });
    // 关闭保存失败时绝不还原：否则开关仍开着（live 仍统一路由），
    // 已迁移会话却被翻回 openai 桶，历史被拆成两半。
    if (saved === false) return;
    // 不再以探测结果短路：还原命令会在迁移锁上排队，等到迁移落盘后
    // 拿到完整账本；确实无账本时由 skippedReason 提示。
    if (!restoreBackup) return;
    try {
      const result = await settingsApi.restoreCodexUnifiedHistory();
      if (result.skippedReason) {
        // unify_toggle_on：还原排队期间开关被重新开启，后端拒绝还原
        toast.info(
          result.skippedReason === "unify_toggle_on"
            ? t("settings.unifyCodexHistoryRestoreSkippedToggleOn")
            : t("settings.unifyCodexHistoryRestoreNothing"),
        );
        return;
      }
      toast.success(
        t("settings.unifyCodexHistoryRestoreCompleted", {
          files: result.restoredJsonlFiles,
          rows: result.restoredStateRows,
        }),
      );
    } catch (error) {
      console.error("Failed to restore codex unified history:", error);
      toast.error(t("settings.unifyCodexHistoryRestoreFailed"));
    }
  };

  const handleDiagnoseHistoryVisibility = async () => {
    setIsDiagnosingHistory(true);
    try {
      const diagnosis = await settingsApi.diagnoseCodexHistoryVisibility();
      setHistoryDiagnosis(diagnosis);
      setShowRepairConfirm(true);
    } catch (error) {
      console.error("Failed to diagnose codex history visibility:", error);
      toast.error(t("settings.codexHistoryVisibilityDiagnoseFailed"));
    } finally {
      setIsDiagnosingHistory(false);
    }
  };

  const handleRepairHistoryVisibility = async () => {
    setShowRepairConfirm(false);
    setIsRepairingHistory(true);
    try {
      const result = await settingsApi.repairCodexHistoryVisibility();
      toast.success(
        t("settings.codexHistoryVisibilityRepairCompleted", {
          rollout: result.changedRolloutFiles,
          sqlite:
            result.sqliteProviderRowsUpdated +
            result.sqliteUserEventRowsUpdated +
            result.sqliteCwdRowsUpdated,
          index: result.sessionIndexRebuilt
            ? result.sessionIndexEntriesWritten
            : 0,
          backup: result.backupDir,
        }),
      );
      if (result.warnings.length > 0) {
        toast.info(result.warnings[0]);
      }
    } catch (error) {
      console.error("Failed to repair codex history visibility:", error);
      toast.error(t("settings.codexHistoryVisibilityRepairFailed"));
    } finally {
      setIsRepairingHistory(false);
    }
  };

  const repairMessage = historyDiagnosis
    ? t("confirm.codexHistoryVisibilityRepair.message", {
        provider: historyDiagnosis.currentProvider,
        rollout: historyDiagnosis.rolloutFilesNeedingProviderSync,
        sqlite:
          historyDiagnosis.sqliteRowsNeedingProviderSync +
          historyDiagnosis.sqliteUserEventRowsNeedingRepair +
          historyDiagnosis.sqliteCwdRowsNeedingRepair,
        roots: historyDiagnosis.workspaceRootsNeedingRepair,
        index: historyDiagnosis.sessionIndexNeedsRebuild
          ? historyDiagnosis.sqliteRows
          : 0,
        warnings: historyDiagnosis.warnings.length,
      })
    : "";

  return (
    <section className="space-y-4">
      <div className="flex items-center gap-2 pb-2 border-b border-border/40">
        <KeyRound className="h-4 w-4 text-primary" />
        <h3 className="text-sm font-medium">{t("settings.codexAuth")}</h3>
      </div>

      <ToggleRow
        icon={<KeyRound className="h-4 w-4 text-emerald-500" />}
        title={t("settings.preserveCodexOfficialAuthOnSwitch")}
        description={t("settings.preserveCodexOfficialAuthOnSwitchDescription")}
        checked={settings.preserveCodexOfficialAuthOnSwitch ?? false}
        onCheckedChange={(value) =>
          onChange({ preserveCodexOfficialAuthOnSwitch: value })
        }
      />

      <ToggleRow
        icon={<History className="h-4 w-4 text-sky-500" />}
        title={t("settings.unifyCodexSessionHistory")}
        description={t("settings.unifyCodexSessionHistoryDescription")}
        checked={settings.unifyCodexSessionHistory ?? false}
        onCheckedChange={handleUnifyHistoryChange}
      />

      <div className="flex items-center justify-between gap-4 rounded-xl border border-border bg-card/50 p-4 transition-colors hover:bg-muted/50">
        <div className="flex items-center gap-3">
          <div className="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg bg-background ring-1 ring-border">
            <Wrench className="h-4 w-4 text-amber-500" />
          </div>
          <div className="space-y-1">
            <p className="text-sm font-medium leading-none">
              {t("settings.codexHistoryVisibilityRepair")}
            </p>
            <p className="text-xs text-muted-foreground">
              {t("settings.codexHistoryVisibilityRepairDescription")}
            </p>
          </div>
        </div>
        <Button
          type="button"
          variant="outline"
          size="sm"
          onClick={handleDiagnoseHistoryVisibility}
          disabled={isDiagnosingHistory || isRepairingHistory}
        >
          {isDiagnosingHistory || isRepairingHistory ? (
            <Loader2 className="h-3.5 w-3.5 animate-spin" />
          ) : (
            <Wrench className="h-3.5 w-3.5" />
          )}
          {isRepairingHistory
            ? t("settings.codexHistoryVisibilityRepairing")
            : t("settings.codexHistoryVisibilityRepairAction")}
        </Button>
      </div>

      <ConfirmDialog
        isOpen={showEnableConfirm}
        title={t("confirm.unifyCodexHistory.title")}
        message={t("confirm.unifyCodexHistory.message")}
        checkboxLabel={t("confirm.unifyCodexHistory.migrateExisting")}
        confirmText={t("confirm.unifyCodexHistory.confirm")}
        onConfirm={handleEnableConfirm}
        onCancel={() => setShowEnableConfirm(false)}
      />

      <ConfirmDialog
        isOpen={showDisableConfirm}
        title={t("confirm.unifyCodexHistoryOff.title")}
        message={t("confirm.unifyCodexHistoryOff.message")}
        checkboxLabel={
          showRestoreOption
            ? t("confirm.unifyCodexHistoryOff.restoreBackup")
            : undefined
        }
        checkboxDefaultChecked
        confirmText={t("confirm.unifyCodexHistoryOff.confirm")}
        onConfirm={(restoreBackup) => void handleDisableConfirm(restoreBackup)}
        onCancel={() => setShowDisableConfirm(false)}
      />

      <ConfirmDialog
        isOpen={showRepairConfirm}
        title={t("confirm.codexHistoryVisibilityRepair.title")}
        message={repairMessage}
        confirmText={t("confirm.codexHistoryVisibilityRepair.confirm")}
        variant="info"
        onConfirm={() => void handleRepairHistoryVisibility()}
        onCancel={() => setShowRepairConfirm(false)}
      />
    </section>
  );
}
