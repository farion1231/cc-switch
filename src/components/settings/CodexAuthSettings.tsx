import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "@/lib/toast";
import type { SettingsFormState } from "@/hooks/useSettings";
import {
  SettingsRow,
  SettingsSwitchRow,
} from "@/components/settings/SettingsLayout";
import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { settingsApi } from "@/lib/api";
import { formatBytes } from "@/components/settings/BackupStorageSection";

interface CodexAuthSettingsProps {
  settings: SettingsFormState;
  /** 返回 false（或 resolve 为 false）表示保存失败；其余返回值视为成功 */
  onChange: (
    updates: Partial<SettingsFormState>,
  ) => void | boolean | Promise<void | boolean>;
  /** 已保存的 Codex 配置目录覆盖（不是输入框里的草稿）：变了就重读会话压缩状态 */
  codexConfigDir?: string;
}

export function CodexAuthSettings({
  settings,
  onChange,
  codexConfigDir,
}: CodexAuthSettingsProps) {
  const { t } = useTranslation();
  const [showEnableConfirm, setShowEnableConfirm] = useState(false);
  const [showDisableConfirm, setShowDisableConfirm] = useState(false);
  const [showManagedConfirm, setShowManagedConfirm] = useState(false);
  const [migratingManaged, setMigratingManaged] = useState(false);
  const [showLegacyOfficialConfirm, setShowLegacyOfficialConfirm] =
    useState(false);
  const [repairingLegacyOfficial, setRepairingLegacyOfficial] = useState(false);
  const [hasUnifyBackup, setHasUnifyBackup] = useState(false);
  const [showRestoreConfirm, setShowRestoreConfirm] = useState(false);
  const [restoringHistory, setRestoringHistory] = useState(false);
  // 会话压缩开关以 config.toml 为准，不走设置表单的自动保存
  const [sessionCompression, setSessionCompression] = useState(false);
  const [sessionCompressionSaving, setSessionCompressionSaving] =
    useState(false);
  const [sessionsBytes, setSessionsBytes] = useState<number | null>(null);

  // 后端按已保存的目录读写 config.toml，同页保存新目录后要跟着重读
  useEffect(() => {
    let cancelled = false;
    setSessionsBytes(null);
    void settingsApi
      .getCodexSessionCompression()
      .catch(() => false)
      .then((enabled) => {
        if (!cancelled) setSessionCompression(enabled);
      });
    void settingsApi
      .getCodexSessionsDiskUsage()
      .catch(() => null)
      .then((bytes) => {
        if (!cancelled) setSessionsBytes(bytes);
      });
    return () => {
      cancelled = true;
    };
  }, [codexConfigDir]);

  useEffect(() => {
    let cancelled = false;
    setHasUnifyBackup(false);
    void settingsApi
      .hasCodexUnifyHistoryBackup()
      .catch(() => false)
      .then((hasBackup) => {
        if (!cancelled) setHasUnifyBackup(hasBackup);
      });
    return () => {
      cancelled = true;
    };
  }, [codexConfigDir, settings.unifyCodexSessionHistory]);

  const handleSessionCompressionChange = async (checked: boolean) => {
    setSessionCompressionSaving(true);
    try {
      setSessionCompression(
        await settingsApi.setCodexSessionCompression(checked),
      );
    } catch (error) {
      console.error("Failed to toggle codex session compression:", error);
      toast.error(t("settings.codexSessionCompressionFailed"));
    } finally {
      setSessionCompressionSaving(false);
    }
  };

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

  const handleManagedMigration = async () => {
    setMigratingManaged(true);
    try {
      const result = await settingsApi.migrateManagedCodexHistory();
      setShowManagedConfirm(false);
      const skippedFiles = result.skippedJsonlFiles ?? 0;
      const skippedRows = result.skippedStateRows ?? 0;
      if (result.skippedReason === "state_db_unlockable") {
        toast.warning(t("settings.codexHistoryStateUnavailable"), {
          closeButton: true,
        });
      } else if (skippedFiles > 0 || skippedRows > 0 || result.retryPending) {
        toast.warning(
          t("settings.codexManagedHistoryPartial", {
            files: result.migratedJsonlFiles,
            rows: result.migratedStateRows,
            skippedFiles,
            skippedRows,
          }) +
            (result.retryPending
              ? " " + t("settings.codexHistoryRetryPending")
              : ""),
          { closeButton: true },
        );
      } else {
        toast.success(
          t("settings.codexManagedHistoryCompleted", {
            files: result.migratedJsonlFiles,
            rows: result.migratedStateRows,
          }),
        );
      }
    } catch (error) {
      setShowManagedConfirm(false);
      toast.error(t("settings.codexHistoryMigrationFailed"), {
        description: t("settings.codexHistoryMigrationRecovery"),
        closeButton: true,
        duration: Infinity,
      });
      console.error("Failed to migrate managed Codex history:", error);
    } finally {
      setMigratingManaged(false);
    }
  };

  // 备份探测可能落后于正在后台进行的迁移（刚勾选迁入就立刻关闭时，
  // 备份尚未产出）。只要本轮勾选过"迁入既有会话"，就必须提供恢复入口；
  // 真正有没有账本交给后端 restore 的 skippedReason 判定。
  const showRestoreOption =
    hasUnifyBackup || (settings.unifyCodexMigrateExisting ?? false);

  const handleLegacyOfficialRepair = async () => {
    if (repairingLegacyOfficial || migratingManaged || restoringHistory) return;
    setRepairingLegacyOfficial(true);
    try {
      const result = await settingsApi.repairLegacyOfficialCodexHistory();
      setShowLegacyOfficialConfirm(false);
      if (result.skippedReason === "legacy_route_in_use") {
        toast.warning(t("settings.codexLegacyOfficialHistoryInUse"), {
          closeButton: true,
        });
      } else if (result.skippedReason === "state_db_unlockable") {
        toast.warning(t("settings.codexHistoryStateUnavailable"), {
          closeButton: true,
        });
      } else if (
        result.skippedJsonlFiles > 0 ||
        result.skippedStateRows > 0 ||
        result.retryPending
      ) {
        toast.warning(
          t("settings.codexLegacyOfficialHistoryPartial", {
            files: result.migratedJsonlFiles,
            rows: result.migratedStateRows,
            skippedFiles: result.skippedJsonlFiles,
            skippedRows: result.skippedStateRows,
          }) +
            (result.retryPending
              ? " " + t("settings.codexHistoryRetryPending")
              : ""),
          { closeButton: true },
        );
      } else {
        toast.success(
          t("settings.codexLegacyOfficialHistoryCompleted", {
            files: result.migratedJsonlFiles,
            rows: result.migratedStateRows,
          }),
        );
      }
    } catch (error) {
      setShowLegacyOfficialConfirm(false);
      toast.error(t("settings.codexHistoryMigrationFailed"), {
        description: t("settings.codexHistoryMigrationRecovery"),
        closeButton: true,
        duration: Infinity,
      });
      console.error("Failed to repair legacy official Codex history:", error);
    } finally {
      setRepairingLegacyOfficial(false);
    }
  };

  const handleRestoreHistory = async () => {
    setShowRestoreConfirm(false);
    setRestoringHistory(true);
    try {
      const result = await settingsApi.restoreCodexUnifiedHistory();
      if (result.skippedReason === "state_db_unlockable") {
        toast.warning(t("settings.codexHistoryStateUnavailable"), {
          closeButton: true,
        });
        return;
      }
      if (result.skippedReason) {
        // unify_toggle_on：还原排队期间开关被重新开启，后端拒绝还原
        toast.info(
          result.skippedReason === "unify_toggle_on"
            ? t("settings.unifyCodexHistoryRestoreSkippedToggleOn")
            : t("settings.unifyCodexHistoryRestoreNothing"),
        );
        return;
      }
      const skippedFiles = result.skippedJsonlFiles ?? 0;
      const skippedRows = result.skippedStateRows ?? 0;
      if (skippedFiles > 0 || skippedRows > 0) {
        toast.warning(
          t("settings.unifyCodexHistoryRestorePartial", {
            files: result.restoredJsonlFiles,
            rows: result.restoredStateRows,
            skippedFiles,
            skippedRows,
          }),
          { closeButton: true },
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
    } finally {
      setRestoringHistory(false);
      void settingsApi
        .hasCodexUnifyHistoryBackup()
        .catch(() => false)
        .then(setHasUnifyBackup);
    }
  };

  const handleDisableConfirm = async (restoreBackup: boolean) => {
    setShowDisableConfirm(false);
    const saved = await onChange({
      unifyCodexSessionHistory: false,
      unifyCodexMigrateExisting: false,
    });
    // A failed save must never restore history while the live route stays unified.
    if (saved === false || !restoreBackup) return;
    await handleRestoreHistory();
  };

  // 设置 → 应用配置 → Codex 卡片里的三行（经典子 agent 开关在 Codex 聚合页签的「聚合设置」里）
  return (
    <>
      <SettingsSwitchRow
        label={t("settings.preserveCodexOfficialAuthOnSwitch")}
        help={{
          title: t("settings.preserveCodexOfficialAuthOnSwitch"),
          body: t("settings.preserveCodexOfficialAuthOnSwitchDescription"),
        }}
        checked={settings.preserveCodexOfficialAuthOnSwitch ?? false}
        onCheckedChange={(value) =>
          onChange({ preserveCodexOfficialAuthOnSwitch: value })
        }
      />

      <SettingsSwitchRow
        label={t("settings.unifyCodexSessionHistory")}
        help={{
          title: t("settings.unifyCodexSessionHistory"),
          body: t("settings.unifyCodexSessionHistoryDescription"),
        }}
        checked={settings.unifyCodexSessionHistory ?? false}
        disabled={
          restoringHistory || migratingManaged || repairingLegacyOfficial
        }
        onCheckedChange={handleUnifyHistoryChange}
      />
      <p role="note" className="px-4 py-3 text-caption text-warning-text">
        {t("settings.unifyCodexDesktopCompatibility")}
      </p>

      {!settings.unifyCodexSessionHistory && hasUnifyBackup && (
        <SettingsRow
          label={t("settings.unifyCodexHistoryRestoreAction")}
          control={
            <Button
              variant="outline"
              size="sm"
              disabled={
                restoringHistory || migratingManaged || repairingLegacyOfficial
              }
              onClick={() => setShowRestoreConfirm(true)}
            >
              {t("settings.unifyCodexHistoryRestoreAction")}
            </Button>
          }
        />
      )}

      {settings.unifyCodexSessionHistory && (
        <SettingsRow
          label={t("settings.codexManagedHistory")}
          help={{
            title: t("settings.codexManagedHistory"),
            body: t("confirm.codexManagedHistory.message"),
          }}
          control={
            <Button
              variant="outline"
              size="sm"
              disabled={
                migratingManaged || restoringHistory || repairingLegacyOfficial
              }
              onClick={() => setShowManagedConfirm(true)}
            >
              {t("settings.codexManagedHistoryAction")}
            </Button>
          }
        />
      )}

      <SettingsRow
        label={t("settings.codexLegacyOfficialHistory")}
        help={{
          title: t("settings.codexLegacyOfficialHistory"),
          body: t("confirm.codexLegacyOfficialHistory.message"),
        }}
        control={
          <Button
            variant="outline"
            size="sm"
            disabled={
              repairingLegacyOfficial || migratingManaged || restoringHistory
            }
            onClick={() => setShowLegacyOfficialConfirm(true)}
          >
            {t("settings.codexLegacyOfficialHistoryAction")}
          </Button>
        }
      />

      {!settings.unifyCodexSessionHistory && hasUnifyBackup && (
        <SettingsRow
          label={t("settings.unifyCodexHistoryRestoreAction")}
          control={
            <Button
              variant="outline"
              size="sm"
              disabled={
                restoringHistory || migratingManaged || repairingLegacyOfficial
              }
              onClick={() => setShowRestoreConfirm(true)}
            >
              {t("settings.unifyCodexHistoryRestoreAction")}
            </Button>
          }
        />
      )}

      {settings.unifyCodexSessionHistory && (
        <SettingsRow
          label={t("settings.codexManagedHistory")}
          help={{
            title: t("settings.codexManagedHistory"),
            body: t("confirm.codexManagedHistory.message"),
          }}
          control={
            <Button
              variant="outline"
              size="sm"
              disabled={
                migratingManaged || restoringHistory || repairingLegacyOfficial
              }
              onClick={() => setShowManagedConfirm(true)}
            >
              {t("settings.codexManagedHistoryAction")}
            </Button>
          }
        />
      )}

      <SettingsRow
        label={t("settings.codexLegacyOfficialHistory")}
        help={{
          title: t("settings.codexLegacyOfficialHistory"),
          body: t("confirm.codexLegacyOfficialHistory.message"),
        }}
        control={
          <Button
            variant="outline"
            size="sm"
            disabled={
              repairingLegacyOfficial || migratingManaged || restoringHistory
            }
            onClick={() => setShowLegacyOfficialConfirm(true)}
          >
            {t("settings.codexLegacyOfficialHistoryAction")}
          </Button>
        }
      />

      <SettingsSwitchRow
        label={t("settings.codexSessionCompression")}
        help={{
          title: t("settings.codexSessionCompression"),
          body: t("settings.codexSessionCompressionDescription"),
        }}
        description={
          sessionsBytes === null
            ? undefined
            : t("settings.codexSessionCompressionUsage", {
                size: formatBytes(sessionsBytes),
              })
        }
        checked={sessionCompression}
        disabled={sessionCompressionSaving}
        onCheckedChange={(value) => void handleSessionCompressionChange(value)}
      />

      <ConfirmDialog
        isOpen={showEnableConfirm}
        title={t("confirm.unifyCodexHistory.title")}
        message={t("confirm.unifyCodexHistory.message")}
        checkboxLabel={t("confirm.unifyCodexHistory.migrateExisting")}
        confirmText={t("confirm.unifyCodexHistory.confirm")}
        variant="info"
        onConfirm={handleEnableConfirm}
        onCancel={() => setShowEnableConfirm(false)}
      />

      <ConfirmDialog
        isOpen={showManagedConfirm}
        title={t("settings.codexManagedHistory")}
        message={t("confirm.codexManagedHistory.message")}
        confirmText={t("settings.codexManagedHistoryAction")}
        variant="info"
        pending={migratingManaged}
        onConfirm={() => void handleManagedMigration()}
        onCancel={() => setShowManagedConfirm(false)}
      />

      <ConfirmDialog
        isOpen={showLegacyOfficialConfirm}
        title={t("settings.codexLegacyOfficialHistory")}
        message={t("confirm.codexLegacyOfficialHistory.message")}
        confirmText={t("settings.codexLegacyOfficialHistoryAction")}
        variant="info"
        pending={repairingLegacyOfficial}
        onConfirm={() => void handleLegacyOfficialRepair()}
        onCancel={() => setShowLegacyOfficialConfirm(false)}
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
        variant="info"
        onConfirm={(restoreBackup) => void handleDisableConfirm(restoreBackup)}
        onCancel={() => setShowDisableConfirm(false)}
      />

      <ConfirmDialog
        isOpen={showRestoreConfirm}
        title={t("settings.unifyCodexHistoryRestoreAction")}
        message={t("settings.unifyCodexHistoryRestoreConfirm")}
        confirmText={t("settings.unifyCodexHistoryRestoreAction")}
        variant="info"
        pending={restoringHistory}
        onConfirm={() => void handleRestoreHistory()}
        onCancel={() => setShowRestoreConfirm(false)}
      />
    </>
  );
}
