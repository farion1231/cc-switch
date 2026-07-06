import { useState, useMemo } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { save } from "@tauri-apps/plugin-dialog";
import { invoke } from "@tauri-apps/api/core";
import { Download, Upload } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { providersApi } from "@/lib/api/providers";
import type { AppId } from "@/lib/api";
import type { Provider } from "@/types";

// 受管应用列表（与设置页其它地方的可见应用保持一致）
const MANAGED_APPS: { id: AppId; labelKey: string }[] = [
  { id: "claude", labelKey: "apps.claude" },
  { id: "claude-desktop", labelKey: "apps.claudeDesktop" },
  { id: "codex", labelKey: "apps.codex" },
  { id: "gemini", labelKey: "apps.gemini" },
  { id: "opencode", labelKey: "apps.opencode" },
  { id: "openclaw", labelKey: "apps.openclaw" },
  { id: "hermes", labelKey: "apps.hermes" },
];

// app id → i18n label key，用于把后端错误码里的 app id 渲染成本地化名称
const APP_LABEL_KEY: Record<string, string> = Object.fromEntries(
  MANAGED_APPS.map((a) => [a.id, a.labelKey]),
);

/**
 * 解析后端 import_providers 返回的错误字符串。
 * 后端对可本地化的错误返回 `CODE:arg1:arg2` 形式的错误码，
 * 前端据此渲染 i18n 文案；无法识别的错误原样返回。
 */
function resolveImportError(
  raw: unknown,
  t: (key: string, opts?: Record<string, unknown>) => string,
): string {
  const msg = typeof raw === "string" ? raw : String(raw);
  if (msg.startsWith("APP_TYPE_MISMATCH:")) {
    const [, sourceId, targetId] = msg.split(":");
    const source = t(APP_LABEL_KEY[sourceId] ?? sourceId, {
      defaultValue: sourceId,
    });
    const target = t(APP_LABEL_KEY[targetId] ?? targetId, {
      defaultValue: targetId,
    });
    return t("provider.importExport.appTypeMismatch", {
      defaultValue: "应用类型不匹配",
      source,
      target,
    });
  }
  return msg;
}

interface ProviderConfigManagementProps {}

/**
 * 设置 → 高级 → 供应商配置管理
 * 按应用维度导入/导出该应用的供应商配置（JSON 文件）。
 */
export function ProviderConfigManagement({}: ProviderConfigManagementProps) {
  const apps = MANAGED_APPS;
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [selectedApp, setSelectedApp] = useState<AppId>(
    apps[0]?.id ?? "claude",
  );
  const [exporting, setExporting] = useState(false);
  const [importing, setImporting] = useState(false);

  // 拉取选中应用的供应商列表，用于导出全选
  const { data: providers } = useQuery<Record<string, Provider>>({
    queryKey: ["providers", selectedApp],
    queryFn: () => providersApi.getAll(selectedApp),
  });

  const providerCount = useMemo(
    () => (providers ? Object.keys(providers).length : 0),
    [providers],
  );

  // 导出：导出该应用全部供应商到用户选择的路径
  const handleExport = async () => {
    if (!providers || providerCount === 0) {
      toast.error(
        t("provider.importExport.noProviders", {
          defaultValue: "没有可导出的供应商",
        }),
      );
      return;
    }

    setExporting(true);
    try {
      const allIds = Object.keys(providers);
      const jsonContent = await providersApi.exportProviders(
        selectedApp,
        allIds,
      );

      const defaultName = `providers-${selectedApp}-${
        new Date().toISOString().split("T")[0]
      }.json`;
      const savePath = await save({
        defaultPath: defaultName,
        filters: [{ name: "JSON", extensions: ["json"] }],
      });
      if (!savePath) return;

      await invoke("write_text_file_to_path", {
        path: savePath,
        content: jsonContent,
      });

      toast.success(
        t("provider.importExport.exportSuccess", {
          defaultValue: `已导出 ${allIds.length} 个供应商配置`,
          count: allIds.length,
        }),
      );
    } catch (error) {
      console.error("Failed to export providers:", error);
      toast.error(
        t("provider.importExport.exportError", {
          defaultValue: "导出供应商配置失败",
        }),
      );
    } finally {
      setExporting(false);
    }
  };

  // 导入：选择 JSON 文件导入到当前选中应用
  const handleImportFile = (event: React.ChangeEvent<HTMLInputElement>) => {
    const file = event.target.files?.[0];
    if (!file) return;
    // 重置 input value 允许重复选择同一文件
    event.target.value = "";

    if (!file.name.endsWith(".json") && file.type !== "application/json") {
      toast.error(
        t("provider.importExport.invalidFileType", {
          defaultValue: "请选择 JSON 文件",
        }),
      );
      return;
    }
    if (file.size > 10 * 1024 * 1024) {
      toast.error(
        t("provider.importExport.fileTooLarge", {
          defaultValue: "文件过大，请选择小于 10MB 的文件",
        }),
      );
      return;
    }

    const reader = new FileReader();
    reader.onload = async (e) => {
      const content = e.target?.result as string;
      try {
        JSON.parse(content);
      } catch {
        toast.error(
          t("provider.importExport.invalidJson", {
            defaultValue: "无效的 JSON 格式",
          }),
        );
        return;
      }

      setImporting(true);
      try {
        const count = await providersApi.importProviders(selectedApp, content);
        toast.success(
          t("provider.importExport.importSuccess", {
            defaultValue: `已导入 ${count} 个供应商配置`,
            count,
          }),
        );
        queryClient.invalidateQueries({
          queryKey: ["providers", selectedApp],
        });
      } catch (error) {
        console.error("Failed to import providers:", error);
        toast.error(
          resolveImportError(error, t) ||
            t("provider.importExport.importError", {
              defaultValue: "导入供应商配置失败",
            }),
        );
      } finally {
        setImporting(false);
      }
    };
    reader.onerror = () => {
      toast.error(
        t("provider.importExport.readError", {
          defaultValue: "读取文件失败",
        }),
      );
    };
    reader.readAsText(file);
  };

  return (
    <div className="space-y-4">
      {/* 应用选择 */}
      <div className="space-y-2">
        <Label htmlFor="provider-config-app-select" className="text-sm">
          {t("provider.importExport.appSelect", {
            defaultValue: "选择应用",
          })}
        </Label>
        <Select
          value={selectedApp}
          onValueChange={(v) => setSelectedApp(v as AppId)}
        >
          <SelectTrigger id="provider-config-app-select" className="w-full">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {apps.map((app) => (
              <SelectItem key={app.id} value={app.id}>
                {t(app.labelKey)}
                {providers && app.id === selectedApp
                  ? ` (${providerCount})`
                  : ""}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <p className="text-xs text-muted-foreground">
          {t("provider.importExport.appHint", {
            defaultValue:
              "导出或导入所选应用的全部供应商配置（含 API Key、Base URL 等敏感信息，请妥善保管）",
          })}
        </p>
      </div>

      {/* 操作按钮 */}
      <div className="flex flex-wrap gap-2">
        <Button
          variant="outline"
          onClick={handleExport}
          disabled={exporting || importing || providerCount === 0}
          className="gap-2"
        >
          <Download className="h-4 w-4" />
          {exporting
            ? t("provider.importExport.exporting", {
                defaultValue: "导出中...",
              })
            : t("provider.importExport.exportAll", {
                defaultValue: "导出全部",
              })}
        </Button>

        <Button
          variant="outline"
          onClick={() =>
            document.getElementById("provider-config-import-input")?.click()
          }
          disabled={exporting || importing}
          className="gap-2"
        >
          <Upload className="h-4 w-4" />
          {importing
            ? t("provider.importExport.importing", {
                defaultValue: "导入中...",
              })
            : t("provider.importExport.importButton", {
                defaultValue: "导入",
              })}
        </Button>
        <input
          id="provider-config-import-input"
          type="file"
          accept=".json"
          className="hidden"
          onChange={handleImportFile}
        />
      </div>
    </div>
  );
}
