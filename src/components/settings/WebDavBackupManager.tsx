import { useState, useEffect } from "react";
import { Button } from "@/components/ui/button";
import { toast } from "sonner";
import {
  Loader2,
  Cloud,
  Upload,
  Download,
  Trash2,
  RefreshCw,
  Clock,
} from "lucide-react";
import { webdavApi, type WebDavConfig } from "@/lib/api/webdav";

interface WebDavBackupManagerProps {
  config: WebDavConfig | null;
  onConfigChange?: () => void;
}

interface BackupItem {
  filename: string;
  timestamp: number;
  dateString: string;
}

export function WebDavBackupManager({
  config,
  onConfigChange,
}: WebDavBackupManagerProps) {
  const [backups, setBackups] = useState<BackupItem[]>([]);
  const [isLoading, setIsLoading] = useState(false);
  const [isExporting, setIsExporting] = useState(false);
  const [isImporting, setIsImporting] = useState<string | null>(null);

  // 解析文件名中的时间信息
  const parseBackupTime = (filename: string): { timestamp: number; dateString: string } | null => {
    // 匹配格式: cc-switch-export-20251222_173835.sql
    const match = filename.match(/(\d{8})_(\d{6})\.sql$/);
    if (!match) return null;

    const dateStr = match[1]; // 20251222
    const timeStr = match[2]; // 173835

    // 解析年月日时分秒
    const year = parseInt(dateStr.substring(0, 4));
    const month = parseInt(dateStr.substring(4, 6));
    const day = parseInt(dateStr.substring(6, 8));
    const hour = parseInt(timeStr.substring(0, 2));
    const minute = parseInt(timeStr.substring(2, 4));
    const second = parseInt(timeStr.substring(4, 6));

    // 创建 Date 对象（UTC时间）
    const date = new Date(Date.UTC(year, month - 1, day, hour, minute, second));
    const timestamp = date.getTime();

    // 获取用户时区并格式化
    const timeZone = Intl.DateTimeFormat().resolvedOptions().timeZone;
    const formatter = new Intl.DateTimeFormat('zh-CN', {
      timeZone,
      year: 'numeric',
      month: '2-digit',
      day: '2-digit',
      hour: '2-digit',
      minute: '2-digit',
      second: '2-digit',
      hour12: false,
    });

    // 格式化为用户时区的时间
    const dateString = formatter.format(date);

    return { timestamp, dateString };
  };

  const loadBackups = async () => {
    if (!config) return;

    setIsLoading(true);
    try {
      const files = await webdavApi.listBackups(config);

      // 解析每个文件的时间信息
      const backupItems: BackupItem[] = files
        .map((filename) => {
          const parsed = parseBackupTime(filename);
          if (parsed) {
            return {
              filename,
              timestamp: parsed.timestamp,
              dateString: parsed.dateString,
            };
          }
          return null;
        })
        .filter((item): item is BackupItem => item !== null)
        .sort((a, b) => b.timestamp - a.timestamp); // 按时间倒序排列，最新的在前

      setBackups(backupItems);
    } catch (error) {
      console.error("Failed to load backups", error);
      toast.error("加载备份列表失败");
    } finally {
      setIsLoading(false);
    }
  };

  useEffect(() => {
    if (config) {
      loadBackups();
    }
  }, [config]);

  const handleExport = async () => {
    if (!config) return;

    setIsExporting(true);
    try {
      const result = await webdavApi.exportToWebDav(config);
      if (result.success) {
        toast.success(`备份已上传到云端: ${result.filename}`, {
          closeButton: true,
        });
        loadBackups(); // 刷新列表
      } else {
        toast.error(result.message || "上传备份失败");
      }
    } catch (error) {
      console.error("Failed to export backup", error);
      toast.error("上传备份失败");
    } finally {
      setIsExporting(false);
    }
  };

  const handleImport = async (filename: string) => {
    if (!config) return;

    setIsImporting(filename);
    try {
      const result = await webdavApi.importFromWebDav(config, filename);
      if (result.success) {
        toast.success("备份已从云端下载并导入", { closeButton: true });
        onConfigChange?.();
      } else {
        toast.error(result.message || "导入备份失败");
      }
    } catch (error) {
      console.error("Failed to import backup", error);
      toast.error("导入备份失败");
    } finally {
      setIsImporting(null);
    }
  };

  const handleDelete = async (filename: string) => {
    if (!config) return;

    if (!confirm(`确定要删除备份文件 "${filename}" 吗？`)) {
      return;
    }

    try {
      const result = await webdavApi.deleteBackup(config, filename);
      if (result.success) {
        toast.success("备份文件已删除", { closeButton: true });
        loadBackups(); // 刷新列表
      } else {
        toast.error(result.message || "删除备份失败");
      }
    } catch (error) {
      console.error("Failed to delete backup", error);
      toast.error("删除备份失败");
    }
  };

  if (!config) {
    return (
      <div className="text-center py-8 text-muted-foreground">
        <Cloud className="h-12 w-12 mx-auto mb-4 opacity-50" />
        <p>请先配置 WebDAV 服务器信息</p>
      </div>
    );
  }

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <h3 className="text-lg font-semibold">云端备份管理</h3>
        <Button
          variant="outline"
          size="sm"
          onClick={loadBackups}
          disabled={isLoading}
        >
          {isLoading ? (
            <Loader2 className="h-4 w-4 animate-spin mr-2" />
          ) : (
            <RefreshCw className="h-4 w-4 mr-2" />
          )}
          刷新
        </Button>
      </div>

      <div className="flex gap-2">
        <Button
          onClick={handleExport}
          disabled={isExporting}
          className="flex-1"
        >
          {isExporting ? (
            <>
              <Loader2 className="h-4 w-4 animate-spin mr-2" />
              上传中...
            </>
          ) : (
            <>
              <Upload className="h-4 w-4 mr-2" />
              上传当前配置到云端
            </>
          )}
        </Button>
      </div>

      <div className="border rounded-lg">
        <div className="p-4 border-b bg-muted/50">
          <h4 className="font-medium">云端备份列表</h4>
        </div>
        <div className="max-h-64 overflow-y-auto">
          {isLoading ? (
            <div className="p-8 text-center">
              <Loader2 className="h-6 w-6 animate-spin mx-auto mb-2" />
              <p className="text-sm text-muted-foreground">加载中...</p>
            </div>
          ) : backups.length === 0 ? (
            <div className="p-8 text-center text-muted-foreground">
              暂无云端备份
            </div>
          ) : (
            <div className="divide-y">
              {backups.map((backup) => (
                <div
                  key={backup.filename}
                  className="p-4 flex items-center justify-between hover:bg-muted/50"
                >
                  <div className="flex-1 min-w-0">
                    <div className="flex items-center gap-2">
                      <Clock className="h-4 w-4 text-muted-foreground flex-shrink-0" />
                      <div className="min-w-0 flex-1">
                        <p className="text-sm font-medium">{backup.dateString}</p>
                        <p className="font-mono text-xs text-muted-foreground truncate">
                          {backup.filename}
                        </p>
                      </div>
                    </div>
                  </div>
                  <div className="flex items-center gap-2 ml-4">
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={() => handleImport(backup.filename)}
                      disabled={isImporting === backup.filename}
                      title="下载并导入此备份"
                    >
                      {isImporting === backup.filename ? (
                        <Loader2 className="h-4 w-4 animate-spin" />
                      ) : (
                        <Download className="h-4 w-4" />
                      )}
                    </Button>
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={() => handleDelete(backup.filename)}
                      title="删除此备份"
                    >
                      <Trash2 className="h-4 w-4" />
                    </Button>
                  </div>
                </div>
              ))}
            </div>
          )}
        </div>
      </div>

      <div className="text-sm text-muted-foreground bg-blue-50 dark:bg-blue-950/20 rounded-lg p-4">
        <p className="font-medium mb-1">提示：</p>
        <ul className="space-y-1">
          <li>• 上传：将当前配置备份到 WebDAV 服务器</li>
          <li>• 下载：从云端恢复配置到本地</li>
          <li>• 删除：永久删除云端备份文件</li>
        </ul>
      </div>
    </div>
  );
}
