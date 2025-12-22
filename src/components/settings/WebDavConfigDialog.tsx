import { useState, useEffect } from "react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { toast } from "sonner";
import { Loader2, Cloud, CheckCircle2, XCircle } from "lucide-react";
import { webdavApi, type WebDavConfig } from "@/lib/api/webdav";


interface WebDavConfigDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onSuccess?: () => void;
}

export function WebDavConfigDialog({
  open,
  onOpenChange,
  onSuccess,
}: WebDavConfigDialogProps) {
  const [config, setConfig] = useState<WebDavConfig>({
    url: "",
    username: "",
    password: "",
    remote_path: "/cc-switch/backups/",
  });
  const [isLoading, setIsLoading] = useState(false);
  const [isTesting, setIsTesting] = useState(false);
  const [testResult, setTestResult] = useState<{
    status: "idle" | "success" | "error";
    message: string;
  }>({ status: "idle", message: "" });

  // 加载现有配置
  useEffect(() => {
    if (open) {
      loadConfig();
    }
  }, [open]);

  const loadConfig = async () => {
    setIsLoading(true);
    try {
      const existingConfig = await webdavApi.getConfig();
      if (existingConfig) {
        setConfig(existingConfig);
      }
    } catch (error) {
      console.error("Failed to load WebDAV config", error);
      toast.error("加载 WebDAV 配置失败");
    } finally {
      setIsLoading(false);
    }
  };

  const handleSave = async () => {
    if (!config.url || !config.username || !config.password) {
      toast.error("请填写完整的 WebDAV 配置信息");
      return;
    }

    setIsLoading(true);
    try {
      const result = await webdavApi.saveConfig(config);
      if (result.success) {
        toast.success("WebDAV 配置已保存", { closeButton: true });
        onSuccess?.();
        onOpenChange(false);
      } else {
        toast.error(result.message || "保存 WebDAV 配置失败");
      }
    } catch (error) {
      console.error("Failed to save WebDAV config", error);
      toast.error("保存 WebDAV 配置失败");
    } finally {
      setIsLoading(false);
    }
  };

  const handleTestConnection = async () => {
    if (!config.url || !config.username || !config.password) {
      toast.error("请先填写完整的配置信息");
      return;
    }

    setIsTesting(true);
    setTestResult({ status: "idle", message: "" });
    try {
      const result = await webdavApi.testConnection(config);
      if (result.success) {
        setTestResult({ status: "success", message: "连接成功" });
        toast.success("WebDAV 连接测试成功", { closeButton: true });
      } else {
        setTestResult({
          status: "error",
          message: result.message || "连接失败",
        });
        toast.error(result.message || "WebDAV 连接测试失败");
      }
    } catch (error) {
      const message =
        error instanceof Error ? error.message : "连接测试失败";
      setTestResult({ status: "error", message });
      toast.error(message);
    } finally {
      setIsTesting(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-[560px] max-h-[90vh] overflow-hidden flex flex-col">
        <DialogHeader className="shrink-0">
          <DialogTitle className="flex items-center gap-2">
            <Cloud className="h-5 w-5 text-blue-500" />
            配置 WebDAV 云端备份
          </DialogTitle>
          <DialogDescription>
            配置 WebDAV 服务器信息以启用云端备份功能
          </DialogDescription>
        </DialogHeader>

        <div className="flex-1 overflow-y-auto pr-2 -mr-2">
          <div className="space-y-5 py-2">
            <div className="space-y-2">
              <Label htmlFor="url" className="text-sm font-medium">
                服务器地址 <span className="text-red-500">*</span>
              </Label>
              <Input
                id="url"
                placeholder="https://your-webdav-server.com/remote.php/dav/files/username/"
                value={config.url}
                onChange={(e) =>
                  setConfig({ ...config, url: e.target.value })
                }
                className="h-11"
              />
            </div>

            <div className="space-y-2">
              <Label htmlFor="username" className="text-sm font-medium">
                用户名 <span className="text-red-500">*</span>
              </Label>
              <Input
                id="username"
                placeholder="your-username"
                value={config.username}
                onChange={(e) =>
                  setConfig({ ...config, username: e.target.value })
                }
                className="h-11"
              />
            </div>

            <div className="space-y-2">
              <Label htmlFor="password" className="text-sm font-medium">
                密码 <span className="text-red-500">*</span>
              </Label>
              <Input
                id="password"
                type="password"
                placeholder="••••••••"
                value={config.password}
                onChange={(e) =>
                  setConfig({ ...config, password: e.target.value })
                }
                className="h-11"
              />
            </div>

            <div className="space-y-2">
              <Label htmlFor="remote_path" className="text-sm font-medium">
                远程路径
              </Label>
              <Input
                id="remote_path"
                placeholder="/cc-switch/backups/"
                value={config.remote_path}
                onChange={(e) =>
                  setConfig({ ...config, remote_path: e.target.value })
                }
                className="h-11"
              />
            </div>

            {/* 连接测试 */}
            <div className="space-y-2 pt-2">
              <Label className="text-sm font-medium">连接测试</Label>
              <Button
                type="button"
                variant="outline"
                onClick={handleTestConnection}
                disabled={isTesting || !config.url || !config.username || !config.password}
                className="w-full h-11"
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

            {/* 帮助信息 - 折叠式 */}
            <div className="bg-muted/30 rounded-lg p-4 text-sm space-y-2">
              <p className="font-medium text-foreground">常用 WebDAV 服务配置示例</p>
              <div className="grid gap-3 text-xs">
                <div className="flex flex-col gap-1">
                  <span className="font-medium text-muted-foreground">坚果云</span>
                  <code className="font-mono text-[11px] bg-background px-2 py-1 rounded border">
                    https://dav.jianguoyun.com/dav/
                  </code>
                </div>
                <div className="flex flex-col gap-1">
                  <span className="font-medium text-muted-foreground">Nextcloud</span>
                  <code className="font-mono text-[11px] bg-background px-2 py-1 rounded border">
                    https://your-server.com/remote.php/dav/files/username/
                  </code>
                </div>
                <div className="flex flex-col gap-1">
                  <span className="font-medium text-muted-foreground">OwnCloud</span>
                  <code className="font-mono text-[11px] bg-background px-2 py-1 rounded border">
                    https://your-server.com/remote.php/webdav/
                  </code>
                </div>
              </div>
            </div>
          </div>
        </div>

        <DialogFooter className="shrink-0 flex gap-3">
          <Button
            type="button"
            variant="outline"
            onClick={() => onOpenChange(false)}
            disabled={isLoading}
            className="h-11"
          >
            取消
          </Button>
          <Button
            onClick={handleSave}
            disabled={isLoading}
            className="h-11 px-8"
          >
            {isLoading ? (
              <>
                <Loader2 className="h-4 w-4 animate-spin mr-2" />
                保存中...
              </>
            ) : (
              "保存配置"
            )}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
