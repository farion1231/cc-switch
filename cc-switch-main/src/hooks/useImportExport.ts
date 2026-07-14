import { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { settingsApi } from "@/lib/api";
import { syncCurrentProvidersLiveSafe } from "@/utils/postChangeSync";

export type ImportStatus =
  | "idle"
  | "importing"
  | "success"
  | "partial-success"
  | "error";

export interface UseImportExportOptions {
  onImportSuccess?: () => void | Promise<void>;
}

export interface UseImportExportResult {
  selectedFile: string;
  status: ImportStatus;
  errorMessage: string | null;
  backupId: string | null;
  isImporting: boolean;
  selectImportFile: () => Promise<void>;
  clearSelection: () => void;
  importConfig: () => Promise<void>;
  exportConfig: () => Promise<void>;
  resetStatus: () => void;
}

export function useImportExport(
  options: UseImportExportOptions = {},
): UseImportExportResult {
  const { t } = useTranslation();
  const { onImportSuccess } = options;

  const [selectedFile, setSelectedFile] = useState("");
  const [status, setStatus] = useState<ImportStatus>("idle");
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const [backupId, setBackupId] = useState<string | null>(null);
  const [isImporting, setIsImporting] = useState(false);

  const clearSelection = useCallback(() => {
    setSelectedFile("");
    setStatus("idle");
    setErrorMessage(null);
    setBackupId(null);
  }, []);

  const selectImportFile = useCallback(async () => {
    try {
      const filePath = await settingsApi.openFileDialog();
      if (filePath) {
        setSelectedFile(filePath);
        setStatus("idle");
        setErrorMessage(null);
      }
    } catch (error) {
      console.error("[useImportExport] Failed to open file dialog", error);
      toast.error(
        t("settings.selectFileFailed", {
          defaultValue: "选择文件失败",
        }),
      );
    }
  }, [t]);

  const importConfig = useCallback(async () => {
    if (!selectedFile) {
      toast.error(
        t("settings.selectFileFailed", {
          defaultValue: "请选择有效的 SQL 备份文件",
        }),
      );
      return;
    }

    if (isImporting) return;

    setIsImporting(true);
    setStatus("importing");
    setErrorMessage(null);

    try {
      const result = await settingsApi.importConfigFromFile(selectedFile);
      if (!result.success) {
        setStatus("error");
        const message =
          result.message ||
          t("settings.configCorrupted", {
            defaultValue: "SQL 文件已损坏或格式不正确",
          });
        setErrorMessage(message);
        toast.error(message);
        return;
      }

      setBackupId(result.backupId ?? null);
      // 导入成功后立即触发外部刷新（与 live 同步结果解耦）
      // - 避免 sync 失败时 UI 不刷新
      // - 避免依赖 setTimeout（组件卸载会取消）
      void onImportSuccess?.();

      const syncResult = await syncCurrentProvidersLiveSafe();
      if (syncResult.ok) {
        setStatus("success");
        toast.success(
          t("settings.importSuccess", {
            defaultValue: "配置导入成功",
          }),
          { closeButton: true },
        );
      } else {
        console.error(
          "[useImportExport] Failed to sync live config",
          syncResult.error,
        );
        setStatus("partial-success");
        toast.warning(
          t("settings.importPartialSuccess", {
            defaultValue:
              "配置已导入，但同步到当前供应商失败。请手动重新选择一次供应商。",
          }),
        );
      }
    } catch (error) {
      console.error("[useImportExport] Failed to import config", error);
      setStatus("error");
      const message =
        error instanceof Error ? error.message : String(error ?? "");
      setErrorMessage(message);
      toast.error(
        t("settings.importFailedError", {
          defaultValue: "导入配置失败: {{message}}",
          message,
        }),
      );
    } finally {
      setIsImporting(false);
    }
  }, [isImporting, onImportSuccess, selectedFile, t]);

  const exportConfig = useCallback(async () => {
    try {
      const now = new Date();
      const stamp = `${now.getFullYear()}${String(now.getMonth() + 1).padStart(2, "0")}${String(now.getDate()).padStart(2, "0")}_${String(now.getHours()).padStart(2, "0")}${String(now.getMinutes()).padStart(2, "0")}${String(now.getSeconds()).padStart(2, "0")}`;
      const defaultName = `cc-switch-export-${stamp}.sql`;
      const destination = await settingsApi.saveFileDialog(defaultName);
      if (!destination) {
        toast.error(
          t("settings.selectFileFailed", {
            defaultValue: "请选择 SQL 备份保存路径",
          }),
        );
        return;
      }

      const result = await settingsApi.exportConfigToFile(destination);
      if (result.success) {
        const displayPath = result.filePath ?? destination;
        toast.success(
          t("settings.configExported", {
            defaultValue: "配置已导出",
          }) + `\n${displayPath}`,
          { closeButton: true },
        );
      } else {
        toast.error(
          t("settings.exportFailed", {
            defaultValue: "导出配置失败",
          }) + (result.message ? `: ${result.message}` : ""),
        );
      }
    } catch (error) {
      console.error("[useImportExport] Failed to export config", error);
      toast.error(
        t("settings.exportFailedError", {
          defaultValue: "导出配置失败: {{message}}",
          message: error instanceof Error ? error.message : String(error ?? ""),
        }),
      );
    }
  }, [t]);

  const resetStatus = useCallback(() => {
    setStatus("idle");
    setErrorMessage(null);
    setBackupId(null);
  }, []);

  return {
    selectedFile,
    status,
    errorMessage,
    backupId,
    isImporting,
    selectImportFile,
    clearSelection,
    importConfig,
    exportConfig,
    resetStatus,
  };
}
