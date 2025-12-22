import { useState, useEffect, useMemo } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Cloud,
  Loader2,
  CheckCircle2,
  XCircle,
  Server,
  User,
  Lock,
  Folder,
  Info,
  FolderOpen,
  Save,
  Database,
  ChevronDown,
} from "lucide-react";
import { webdavApi, type WebDavConfig } from "@/lib/api/webdav";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";
import type { ImportStatus } from "@/hooks/useImportExport";
import { WebDavBackupManager } from "./WebDavBackupManager";

interface WebDavSettingsProps {
  importStatus?: ImportStatus;
  selectedFile?: string;
  errorMessage?: string | null;
  backupId?: string | null;
  isImporting?: boolean;
  onSelectFile?: () => Promise<void>;
  onImport?: () => Promise<void>;
  onExport?: () => Promise<void>;
  onClear?: () => void;
}

export function WebDavSettings({
  importStatus = "idle",
  selectedFile = "",
  errorMessage = null,
  backupId = null,
  isImporting = false,
  onSelectFile,
  onImport,
  onExport,
  onClear,
}: WebDavSettingsProps) {
  const { t } = useTranslation();
  const [config, setConfig] = useState<WebDavConfig | null>(null);
  const [isLoading, setIsLoading] = useState(true);

  // 表单状态
  const [isEditing, setIsEditing] = useState(false);
  const [isTesting, setIsTesting] = useState(false);
  const [testResult, setTestResult] = useState<{
    status: "idle" | "success" | "error";
    message: string;
  }>({ status: "idle", message: "" });
  const [formData, setFormData] = useState<WebDavConfig>({
    url: "",
    username: "",
    password: "",
    remote_path: "/cc-switch/backups/",
  });

  useEffect(() => {
    loadConfig();
  }, []);

  const loadConfig = async () => {
    setIsLoading(true);
    try {
      const existingConfig = await webdavApi.getConfig();
      setConfig(existingConfig);
      if (existingConfig) {
        setFormData(existingConfig);
      }
    } catch (error) {
      console.error("Failed to load WebDAV config", error);
    } finally {
      setIsLoading(false);
    }
  };

  const selectedFileName = useMemo(() => {
    if (!selectedFile) return "";
    const segments = selectedFile.split(/[\\/]/);
    return segments[segments.length - 1] || selectedFile;
  }, [selectedFile]);

  const handleTestConnection = async () => {
    if (!formData.url || !formData.username || !formData.password) {
      toast.error("请先填写完整的配置信息");
      return;
    }

    setIsTesting(true);
    setTestResult({ status: "idle", message: "" });

    try {
      const result = await webdavApi.testConnection(formData);
      if (result.success) {
        setTestResult({
          status: "success",
          message: result.message || "连接成功",
        });
        toast.success("WebDAV 连接测试成功", { closeButton: true });
      } else {
        setTestResult({
          status: "error",
          message: result.message || "连接失败",
        });
        toast.error(result.message || "WebDAV 连接测试失败");
      }
    } catch (error) {
      const message = error instanceof Error ? error.message : "连接测试失败";
      setTestResult({ status: "error", message });
      toast.error(message);
    } finally {
      setIsTesting(false);
    }
  };

  const handleSave = async () => {
    if (!formData.url || !formData.username || !formData.password) {
      toast.error("请填写完整的 WebDAV 配置信息");
      return;
    }

    setIsLoading(true);
    try {
      const result = await webdavApi.saveConfig(formData);
      if (result.success) {
        setConfig(formData);
        setIsEditing(false);
        toast.success("WebDAV 配置已保存", { closeButton: true });
        loadConfig();
      } else {
        toast.error(result.message || "保存配置失败");
      }
    } catch (error) {
      const message = error instanceof Error ? error.message : "保存配置失败";
      toast.error(message);
    } finally {
      setIsLoading(false);
    }
  };

  if (isLoading) {
    return (
      <section className="space-y-4">
        <div className="flex items-center gap-2 pb-2 border-b border-border/40">
          <Cloud className="h-4 w-4 text-primary" />
          <h3 className="text-sm font-medium">备份设置</h3>
        </div>
        <div className="flex items-center justify-center py-4">
          <Loader2 className="h-5 w-5 animate-spin text-muted-foreground" />
        </div>
      </section>
    );
  }

  return (
    <div className="space-y-4">
      {/* 本地备份 */}
      <div className="rounded-xl border border-border bg-card/50 p-5 space-y-4">
        <div className="flex items-center gap-2">
          <Database className="h-4 w-4 text-blue-500" />
          <h3 className="text-base font-semibold">本地备份</h3>
        </div>
        <p className="text-sm text-muted-foreground">
          {t("settings.importExportHint")}
        </p>

        {onSelectFile && onImport && onExport && onClear ? (
          <div className="grid grid-cols-2 gap-4 items-stretch">
            {/* Import Button */}
            <div className="relative">
              <Button
                type="button"
                className={`w-full h-auto py-3 px-4 bg-blue-500 hover:bg-blue-600 dark:bg-blue-600 dark:hover:bg-blue-700 text-white ${
                  selectedFile && !isImporting ? "flex-col items-start" : "items-center"
                }`}
                onClick={!selectedFile ? onSelectFile : onImport}
                disabled={isImporting}
              >
                <div className="flex items-center gap-2 w-full justify-center">
                  {isImporting ? (
                    <Loader2 className="h-4 w-4 animate-spin flex-shrink-0" />
                  ) : selectedFile ? (
                    <CheckCircle2 className="h-4 w-4 flex-shrink-0" />
                  ) : (
                    <FolderOpen className="h-4 w-4 flex-shrink-0" />
                  )}
                  <span className="font-medium">
                    {isImporting
                      ? t("settings.importing")
                      : selectedFile
                        ? t("settings.import")
                        : t("settings.selectConfigFile")}
                  </span>
                </div>
                {selectedFile && !isImporting && (
                  <div className="mt-2 w-full text-left">
                    <p className="text-xs font-mono text-white/80 truncate">
                      📄 {selectedFileName}
                    </p>
                  </div>
                )}
              </Button>
              {selectedFile && (
                <button
                  type="button"
                  onClick={onClear}
                  className="absolute -top-2 -right-2 h-6 w-6 rounded-full bg-red-500 hover:bg-red-600 text-white flex items-center justify-center shadow-lg transition-colors z-10"
                  aria-label={t("common.clear")}
                >
                  <XCircle className="h-4 w-4" />
                </button>
              )}
            </div>

            {/* Export Button */}
            <div>
              <Button
                type="button"
                className="w-full h-full py-3 px-4 bg-blue-500 hover:bg-blue-600 dark:bg-blue-600 dark:hover:bg-blue-700 text-white items-center"
                onClick={onExport}
              >
                <Save className="mr-2 h-4 w-4" />
                {t("settings.exportConfig")}
              </Button>
            </div>
          </div>
        ) : (
          <p className="text-sm text-muted-foreground">
            本地备份功能需要在主组件中启用
          </p>
        )}

        {/* 导入状态消息 */}
        {onSelectFile && onImport && onExport && onClear && (
          <ImportStatusMessage
            status={importStatus}
            errorMessage={errorMessage}
            backupId={backupId}
          />
        )}
      </div>

      {/* WebDAV 云端备份 */}
      <div className="rounded-xl border border-border bg-card/50 p-5 space-y-5">
        <div className="flex items-center gap-2">
          <Cloud className="h-5 w-5 text-blue-500" />
          <h3 className="text-base font-semibold">WebDAV 云端备份</h3>
        </div>

        {/* 配置提示框 */}
        <div className="flex gap-3 p-3 rounded-lg bg-blue-50/50 dark:bg-blue-950/20 border border-blue-200/50 dark:border-blue-800/30">
          <Info className="h-4 w-4 text-blue-500 mt-0.5 flex-shrink-0" />
          <div className="text-sm text-muted-foreground">
            <p className="font-medium text-blue-700 dark:text-blue-400 mb-1">
              支持的 WebDAV 服务
            </p>
            <p>
              本功能支持 Nextcloud、OwnCloud、坚果云等 WebDAV
              云存储服务。请确保您的服务器开启了 WebDAV 功能。
            </p>
          </div>
        </div>

        {/* WebDAV 配置卡片 */}
        <div
          className="rounded-xl border border-border bg-card/50 p-4 transition-colors hover:bg-muted/50"
        >
          <div
            className="flex items-center gap-3 cursor-pointer"
            onClick={() => {
              // 如果未配置,点击无效(表单已显示)
              // 如果已配置且未编辑,点击展开表单
              // 如果已配置且已编辑,点击收起表单
              if (config) {
                setIsEditing(!isEditing);
                setTestResult({ status: "idle", message: "" });
              }
            }}
          >
            <div className="flex h-10 w-10 items-center justify-center rounded-lg bg-background ring-1 ring-border">
              <Cloud className="h-5 w-5 text-blue-500" />
            </div>
            <div className="flex-1">
              <h4 className="text-sm font-semibold text-foreground">
                WebDAV 配置
              </h4>
              <p className="text-xs text-muted-foreground mt-0.5">
                配置 WebDAV 服务器信息
              </p>
            </div>
            <div className="flex items-center gap-2">
              {config && !isEditing && (
                <>
                  <div className="flex items-center gap-1 px-2 py-1 rounded-md bg-green-50 dark:bg-green-950/30 text-green-600 dark:text-green-400 text-xs">
                    <CheckCircle2 className="h-3 w-3" />
                    <span>已配置</span>
                  </div>
                  <ChevronDown className="h-4 w-4 text-muted-foreground" />
                </>
              )}
              {config && isEditing && (
                <button
                  className="p-1 rounded hover:bg-muted/50 transition-colors"
                  onClick={(e) => {
                    // 阻止冒泡,但实际上不需要,因为父元素会处理
                    e.stopPropagation();
                    setIsEditing(false);
                    setFormData(config);
                    setTestResult({ status: "idle", message: "" });
                  }}
                  title="收起配置"
                >
                  <ChevronDown className="h-4 w-4 text-muted-foreground rotate-180 transition-transform" />
                </button>
              )}
              {!config && (
                <Button
                  variant="default"
                  size="sm"
                  onClick={(e) => {
                    e.stopPropagation();
                    setIsEditing(true);
                    setTestResult({ status: "idle", message: "" });
                  }}
                  disabled={isLoading}
                  className="min-w-[80px]"
                >
                  {isLoading ? (
                    <Loader2 className="h-4 w-4 animate-spin" />
                  ) : (
                    "配置"
                  )}
                </Button>
              )}
            </div>
          </div>
        </div>

        {/* 配置表单 */}
        {(config === null || isEditing) && (
          <div className="space-y-4 pl-14 -mt-4">
            {/* 表单字段 */}
            <div className="space-y-4">
              <div className="space-y-2">
                <Label className="text-sm font-medium flex items-center gap-2">
                  <Server className="h-4 w-4 text-muted-foreground" />
                  服务器地址 <span className="text-red-500">*</span>
                </Label>
                <Input
                  id="url"
                  placeholder="https://your-webdav-server.com/remote.php/dav/files/username/"
                  value={formData.url}
                  onChange={(e) =>
                    setFormData({ ...formData, url: e.target.value })
                  }
                  className="h-10"
                />
              </div>

              <div className="space-y-2">
                <Label className="text-sm font-medium flex items-center gap-2">
                  <User className="h-4 w-4 text-muted-foreground" />
                  用户名 <span className="text-red-500">*</span>
                </Label>
                <Input
                  id="username"
                  placeholder="请输入 WebDAV 用户名"
                  value={formData.username}
                  onChange={(e) =>
                    setFormData({ ...formData, username: e.target.value })
                  }
                  className="h-10"
                />
              </div>

              <div className="space-y-2">
                <Label className="text-sm font-medium flex items-center gap-2">
                  <Lock className="h-4 w-4 text-muted-foreground" />
                  密码 <span className="text-red-500">*</span>
                </Label>
                <Input
                  id="password"
                  type="password"
                  placeholder="请输入 WebDAV 密码"
                  value={formData.password}
                  onChange={(e) =>
                    setFormData({ ...formData, password: e.target.value })
                  }
                  className="h-10"
                />
              </div>

              <div className="space-y-2">
                <Label className="text-sm font-medium flex items-center gap-2">
                  <Folder className="h-4 w-4 text-muted-foreground" />
                  远程路径
                </Label>
                <Input
                  id="remote_path"
                  placeholder="/cc-switch/backups/"
                  value={formData.remote_path}
                  onChange={(e) =>
                    setFormData({ ...formData, remote_path: e.target.value })
                  }
                  className="h-10"
                />
                <p className="text-xs text-muted-foreground">
                  云端存储路径，例如 /cc-switch/backups/
                </p>
              </div>
            </div>

            {/* 测试连接 */}
            <div className="space-y-2">
              <Label className="text-sm font-medium">连接测试</Label>
              <Button
                type="button"
                variant="outline"
                onClick={handleTestConnection}
                disabled={
                  isTesting ||
                  !formData.url ||
                  !formData.username ||
                  !formData.password
                }
                className="w-full h-10"
              >
                {isTesting ? (
                  <>
                    <Loader2 className="h-4 w-4 animate-spin mr-2" />
                    测试中...
                  </>
                ) : (
                  "测试连接"
                )}
              </Button>
              {testResult.status !== "idle" && (
                <div className="flex items-center gap-2 px-1">
                  {testResult.status === "success" ? (
                    <>
                      <CheckCircle2 className="h-4 w-4 text-green-500 flex-shrink-0" />
                      <span className="text-sm text-green-600">
                        {testResult.message}
                      </span>
                    </>
                  ) : (
                    <>
                      <XCircle className="h-4 w-4 text-red-500 flex-shrink-0" />
                      <span className="text-sm text-red-600">
                        {testResult.message}
                      </span>
                    </>
                  )}
                </div>
              )}
            </div>

            {/* 操作按钮 */}
            <div className="flex gap-3 pt-2">
              {isEditing && (
                <Button
                  variant="outline"
                  onClick={() => {
                    setIsEditing(false);
                    setFormData(config || formData);
                    setTestResult({ status: "idle", message: "" });
                  }}
                  className="flex-1 h-10"
                >
                  取消
                </Button>
              )}
              <Button
                onClick={() => {
                  if (!config || (config && !isEditing)) {
                    setIsEditing(true);
                    setTestResult({ status: "idle", message: "" });
                  } else if (isEditing) {
                    handleSave();
                  }
                }}
                disabled={isLoading}
                className={`flex-1 h-10 ${
                  config && !isEditing ? "bg-green-500 hover:bg-green-600" : ""
                }`}
              >
                {isLoading ? (
                  <>
                    <Loader2 className="h-4 w-4 animate-spin mr-2" />
                    保存中...
                  </>
                ) : config && !isEditing ? (
                  <>
                    <Save className="h-4 w-4 mr-2" />
                    保存
                  </>
                ) : (
                  "保存"
                )}
              </Button>
            </div>
          </div>
        )}

        {/* 上传和列表 */}
        {config && (
          <div className="pl-14 -mt-4">
            <WebDavBackupManager
              config={config}
              onConfigChange={loadConfig}
            />
          </div>
        )}
      </div>
    </div>
  );
}

interface ImportStatusMessageProps {
  status: ImportStatus;
  errorMessage: string | null;
  backupId: string | null;
}

function ImportStatusMessage({
  status,
  errorMessage,
  backupId,
}: ImportStatusMessageProps) {
  const { t } = useTranslation();

  if (status === "idle") {
    return null;
  }

  const baseClass =
    "flex items-start gap-3 rounded-xl border p-4 text-sm leading-relaxed backdrop-blur-sm";

  if (status === "importing") {
    return (
      <div
        className={`${baseClass} border-blue-500/30 bg-blue-500/10 text-blue-600 dark:text-blue-400`}
      >
        <Loader2 className="mt-0.5 h-5 w-5 flex-shrink-0 animate-spin" />
        <div>
          <p className="font-semibold">{t("settings.importing")}</p>
          <p className="text-blue-600/80 dark:text-blue-400/80">
            {t("common.loading")}
          </p>
        </div>
      </div>
    );
  }

  if (status === "success") {
    return (
      <div
        className={`${baseClass} border-green-500/30 bg-green-500/10 text-green-700 dark:text-green-400`}
      >
        <CheckCircle2 className="mt-0.5 h-5 w-5 flex-shrink-0" />
        <div>
          <p className="font-semibold">{t("settings.import.success")}</p>
          {backupId && (
            <p className="text-green-600/80 dark:text-green-400/80">
              {t("settings.import.success.backupId", { id: backupId })}
            </p>
          )}
        </div>
      </div>
    );
  }

  // Error state
  return (
    <div
      className={`${baseClass} border-red-500/30 bg-red-500/10 text-red-700 dark:text-red-400`}
    >
      <XCircle className="mt-0.5 h-5 w-5 flex-shrink-0" />
      <div>
        <p className="font-semibold">{t("settings.import.failed")}</p>
        <p className="text-red-600/80 dark:text-red-400/80">
          {errorMessage || t("common.error")}
        </p>
      </div>
    </div>
  );
}
