import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Download, Upload, FileJson } from "lucide-react";
import { toast } from "sonner";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Checkbox } from "@/components/ui/checkbox";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import type { Provider } from "@/types";
import type { AppId } from "@/lib/api";

interface ImportExportDialogProps {
  isOpen: boolean;
  onClose: () => void;
  appId: AppId;
  providers: Record<string, Provider>;
  onExport: (providerIds: string[]) => Promise<void>;
  onImport: (jsonContent: string) => Promise<void>;
}

export function ImportExportDialog({
  isOpen,
  onClose,
  appId,
  providers,
  onExport,
  onImport,
}: ImportExportDialogProps) {
  const { t } = useTranslation();
  const [selectedProviders, setSelectedProviders] = useState<Set<string>>(
    new Set(),
  );
  const [importJson, setImportJson] = useState("");
  const [exporting, setExporting] = useState(false);
  const [importing, setImporting] = useState(false);

  const providerList = Object.values(providers);

  const handleToggleProvider = (providerId: string) => {
    const newSelected = new Set(selectedProviders);
    if (newSelected.has(providerId)) {
      newSelected.delete(providerId);
    } else {
      newSelected.add(providerId);
    }
    setSelectedProviders(newSelected);
  };

  const handleSelectAll = () => {
    if (selectedProviders.size === providerList.length) {
      setSelectedProviders(new Set());
    } else {
      setSelectedProviders(new Set(providerList.map((p) => p.id)));
    }
  };

  const handleExport = async () => {
    if (selectedProviders.size === 0) return;

    setExporting(true);
    try {
      await onExport(Array.from(selectedProviders));
      setSelectedProviders(new Set());
    } finally {
      setExporting(false);
    }
  };

  const handleImport = async () => {
    if (!importJson.trim()) return;

    setImporting(true);
    try {
      await onImport(importJson);
      setImportJson("");
    } finally {
      setImporting(false);
    }
  };

  const handleFileUpload = (event: React.ChangeEvent<HTMLInputElement>) => {
    const file = event.target.files?.[0];
    if (!file) return;

    // Validate file type
    if (!file.name.endsWith(".json") && file.type !== "application/json") {
      toast.error(
        t("provider.importExport.invalidFileType", {
          defaultValue: "请选择 JSON 文件",
        }),
      );
      return;
    }

    // Validate file size (10MB limit)
    if (file.size > 10 * 1024 * 1024) {
      toast.error(
        t("provider.importExport.fileTooLarge", {
          defaultValue: "文件过大，请选择小于 10MB 的文件",
        }),
      );
      return;
    }

    const reader = new FileReader();
    reader.onload = (e) => {
      const content = e.target?.result as string;
      try {
        JSON.parse(content); // Validate JSON format
        setImportJson(content);
      } catch (error) {
        toast.error(
          t("provider.importExport.invalidJson", {
            defaultValue: "无效的 JSON 格式",
          }),
        );
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

  const handleClose = () => {
    if (!exporting && !importing) {
      onClose();
      setSelectedProviders(new Set());
      setImportJson("");
    }
  };

  return (
    <Dialog open={isOpen} onOpenChange={handleClose}>
      <DialogContent className="sm:max-w-[600px] max-h-[80vh] flex flex-col">
        <DialogHeader>
          <DialogTitle>
            {t("provider.importExport.title", {
              defaultValue: "导入/导出供应商配置",
            })}
          </DialogTitle>
          <DialogDescription>
            {t("provider.importExport.description", {
              defaultValue: "导出供应商配置为 JSON 文件，或从文件导入配置",
            })}
          </DialogDescription>
        </DialogHeader>

        <Tabs
          defaultValue="export"
          className="flex-1 flex flex-col min-h-0 p-2"
        >
          <TabsList className="grid w-full grid-cols-2">
            <TabsTrigger value="export">
              <Download className="h-4 w-4 mr-2" />
              {t("provider.importExport.export", { defaultValue: "导出" })}
            </TabsTrigger>
            <TabsTrigger value="import">
              <Upload className="h-4 w-4 mr-2" />
              {t("provider.importExport.import", { defaultValue: "导入" })}
            </TabsTrigger>
          </TabsList>

          <TabsContent
            value="export"
            className="flex-1 flex flex-col space-y-4 min-h-0"
          >
            {/* 全选选项 */}
            <div className="flex items-center gap-2 p-2 border-b">
              <Checkbox
                id="select-all-export"
                checked={selectedProviders.size === providerList.length}
                onCheckedChange={handleSelectAll}
              />
              <Label
                htmlFor="select-all-export"
                className="text-sm font-medium cursor-pointer"
              >
                {t("provider.importExport.selectAll", {
                  defaultValue: "全选",
                })}
              </Label>
            </div>

            {/* 供应商列表 */}
            <div className="flex-1 overflow-y-auto space-y-2 pr-2">
              {providerList.length === 0 ? (
                <div className="text-sm text-muted-foreground text-center py-8">
                  {t("provider.importExport.noProviders", {
                    defaultValue: "没有可导出的供应商",
                  })}
                </div>
              ) : (
                providerList.map((provider) => (
                  <div
                    key={provider.id}
                    className="flex items-center gap-2 p-3 rounded-md hover:bg-accent/50 cursor-pointer border"
                    onClick={() => handleToggleProvider(provider.id)}
                  >
                    <Checkbox
                      id={`export-${provider.id}`}
                      checked={selectedProviders.has(provider.id)}
                      onCheckedChange={() => handleToggleProvider(provider.id)}
                    />
                    <Label
                      htmlFor={`export-${provider.id}`}
                      className="text-sm cursor-pointer flex-1"
                    >
                      <div className="font-medium">{provider.name}</div>
                      {provider.notes && (
                        <div className="text-xs text-muted-foreground mt-1">
                          {provider.notes}
                        </div>
                      )}
                    </Label>
                  </div>
                ))
              )}
            </div>

            {selectedProviders.size > 0 && (
              <p className="text-xs text-muted-foreground">
                {t("provider.importExport.selectedCount", {
                  defaultValue: `已选择 ${selectedProviders.size} 个供应商`,
                  count: selectedProviders.size,
                })}
              </p>
            )}

            <DialogFooter>
              <Button variant="outline" onClick={handleClose}>
                {t("common.cancel", { defaultValue: "取消" })}
              </Button>
              <Button
                onClick={handleExport}
                disabled={selectedProviders.size === 0 || exporting}
              >
                {exporting ? (
                  <>
                    <div className="h-4 w-4 animate-spin rounded-full border-2 border-current border-t-transparent mr-2" />
                    {t("provider.importExport.exporting", {
                      defaultValue: "导出中...",
                    })}
                  </>
                ) : (
                  <>
                    <Download className="h-4 w-4 mr-2" />
                    {t("provider.importExport.exportButton", {
                      defaultValue: `导出 ${selectedProviders.size} 个`,
                      count: selectedProviders.size,
                    })}
                  </>
                )}
              </Button>
            </DialogFooter>
          </TabsContent>

          <TabsContent
            value="import"
            className="flex-1 flex flex-col p-2 space-y-4 min-h-0"
          >
            <div className="space-y-2">
              <Label htmlFor="import-file">
                {t("provider.importExport.selectFile", {
                  defaultValue: "选择文件",
                })}
              </Label>
              <div className="flex gap-2">
                <Button
                  variant="outline"
                  className="flex-1"
                  onClick={() =>
                    document.getElementById("import-file-input")?.click()
                  }
                >
                  <FileJson className="h-4 w-4 mr-2" />
                  {t("provider.importExport.chooseFile", {
                    defaultValue: "选择 JSON 文件",
                  })}
                </Button>
                <input
                  id="import-file-input"
                  type="file"
                  accept=".json"
                  className="hidden"
                  onChange={handleFileUpload}
                />
              </div>
            </div>

            <div className="flex-1 flex flex-col space-y-2 min-h-0">
              <Label htmlFor="import-json">
                {t("provider.importExport.jsonContent", {
                  defaultValue: "或粘贴 JSON 内容",
                })}
              </Label>
              <Textarea
                id="import-json"
                value={importJson}
                onChange={(e) => setImportJson(e.target.value)}
                placeholder={t("provider.importExport.jsonPlaceholder", {
                  defaultValue: "粘贴导出的 JSON 配置...",
                })}
                className="flex-1 font-mono text-xs resize-none"
              />
            </div>

            <DialogFooter>
              <Button variant="outline" onClick={handleClose}>
                {t("common.cancel", { defaultValue: "取消" })}
              </Button>
              <Button
                onClick={handleImport}
                disabled={!importJson.trim() || importing}
              >
                {importing ? (
                  <>
                    <div className="h-4 w-4 animate-spin rounded-full border-2 border-current border-t-transparent mr-2" />
                    {t("provider.importExport.importing", {
                      defaultValue: "导入中...",
                    })}
                  </>
                ) : (
                  <>
                    <Upload className="h-4 w-4 mr-2" />
                    {t("provider.importExport.importButton", {
                      defaultValue: "导入",
                    })}
                  </>
                )}
              </Button>
            </DialogFooter>
          </TabsContent>
        </Tabs>
      </DialogContent>
    </Dialog>
  );
}
