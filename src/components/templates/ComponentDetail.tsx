import { useState } from "react";
import { useTranslation } from "react-i18next";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import {
  Download,
  Trash2,
  Loader2,
  ExternalLink,
  FileCode,
  FolderGit2,
  Tag,
  Clock,
} from "lucide-react";
import { settingsApi } from "@/lib/api";
import {
  useTemplateComponent,
  useComponentPreview,
} from "@/lib/query/template";
import type { AppType } from "@/lib/api/config";

interface ComponentDetailProps {
  componentId: number;
  selectedApp: AppType;
  onClose: () => void;
  onInstall: (id: number, name: string) => Promise<void>;
  onUninstall: (id: number, name: string) => Promise<void>;
}

// 组件类型图标映射
const componentTypeIcons: Record<string, string> = {
  agent: "🤖",
  command: "⚡",
  mcp: "🔌",
  setting: "⚙️",
  hook: "🪝",
  skill: "💡",
};

export function ComponentDetail({
  componentId,
  onClose,
  onInstall,
  onUninstall,
}: ComponentDetailProps) {
  const { t } = useTranslation();
  const [loading, setLoading] = useState(false);

  const { data: component, isLoading: componentLoading } =
    useTemplateComponent(componentId);
  const { data: preview, isLoading: previewLoading } =
    useComponentPreview(componentId);

  const handleInstall = async () => {
    if (!component || component.id === null) return;
    setLoading(true);
    try {
      await onInstall(component.id, component.name);
    } finally {
      setLoading(false);
    }
  };

  const handleUninstall = async () => {
    if (!component || component.id === null) return;
    setLoading(true);
    try {
      await onUninstall(component.id, component.name);
    } finally {
      setLoading(false);
    }
  };

  const handleOpenGithub = async () => {
    if (component?.readmeUrl) {
      try {
        await settingsApi.openExternal(component.readmeUrl);
      } catch (error) {
        console.error("Failed to open URL:", error);
      }
    }
  };

  if (componentLoading || !component) {
    return (
      <Dialog open={true} onOpenChange={onClose}>
        <DialogContent className="max-w-4xl max-h-[80vh] overflow-hidden">
          <div className="flex items-center justify-center h-64">
            <Loader2 className="h-8 w-8 animate-spin text-muted-foreground" />
          </div>
        </DialogContent>
      </Dialog>
    );
  }

  const typeIcon = componentTypeIcons[component.componentType] || "📦";

  return (
    <Dialog open={true} onOpenChange={onClose}>
      <DialogContent className="max-w-4xl max-h-[80vh] overflow-hidden flex flex-col">
        <DialogHeader>
          <div className="flex items-start justify-between gap-4">
            <div className="flex-1 min-w-0">
              <div className="flex items-center gap-2 mb-2">
                <span className="text-3xl">{typeIcon}</span>
                <Badge variant="outline" className="text-xs">
                  {t(`templates.type.${component.componentType}`, {
                    defaultValue: component.componentType,
                  })}
                </Badge>
                {component.category && (
                  <Badge variant="secondary" className="text-xs">
                    {component.category}
                  </Badge>
                )}
              </div>
              <DialogTitle className="text-2xl">{component.name}</DialogTitle>
              <DialogDescription className="text-sm mt-2">
                {component.description ||
                  t("templates.noDescription", { defaultValue: "暂无描述" })}
              </DialogDescription>
            </div>
            {component.installed && (
              <Badge
                variant="default"
                className="shrink-0 bg-green-600/90 text-white border-0"
              >
                {t("templates.installed", { defaultValue: "已安装" })}
              </Badge>
            )}
          </div>
        </DialogHeader>

        <div className="flex-1 overflow-hidden">
          <Tabs defaultValue="info" className="h-full flex flex-col">
            <TabsList>
              <TabsTrigger value="info">
                {t("templates.detail.info", { defaultValue: "信息" })}
              </TabsTrigger>
              <TabsTrigger value="preview">
                <FileCode className="h-3.5 w-3.5 mr-1.5" />
                {t("templates.detail.preview", { defaultValue: "预览" })}
              </TabsTrigger>
            </TabsList>

            <TabsContent value="info" className="flex-1 overflow-y-auto mt-4">
              <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
                {/* 类型 */}
                <div className="flex items-center gap-3 p-3 rounded-lg bg-muted/30">
                  <div className="flex items-center justify-center w-10 h-10 rounded-lg bg-primary/10">
                    <Tag className="h-5 w-5 text-primary" />
                  </div>
                  <div className="flex-1 min-w-0">
                    <p className="text-xs text-muted-foreground">
                      {t("templates.detail.type", { defaultValue: "类型" })}
                    </p>
                    <p className="text-sm font-medium truncate">
                      {t(`templates.type.${component.componentType}`, {
                        defaultValue: component.componentType,
                      })}
                    </p>
                  </div>
                </div>

                {/* 分类 */}
                {component.category && (
                  <div className="flex items-center gap-3 p-3 rounded-lg bg-muted/30">
                    <div className="flex items-center justify-center w-10 h-10 rounded-lg bg-primary/10">
                      <Tag className="h-5 w-5 text-primary" />
                    </div>
                    <div className="flex-1 min-w-0">
                      <p className="text-xs text-muted-foreground">
                        {t("templates.detail.category", {
                          defaultValue: "分类",
                        })}
                      </p>
                      <p className="text-sm font-medium truncate">
                        {component.category}
                      </p>
                    </div>
                  </div>
                )}

                {/* 仓库 */}
                <div className="flex items-center gap-3 p-3 rounded-lg bg-muted/30">
                  <div className="flex items-center justify-center w-10 h-10 rounded-lg bg-primary/10">
                    <FolderGit2 className="h-5 w-5 text-primary" />
                  </div>
                  <div className="flex-1 min-w-0">
                    <p className="text-xs text-muted-foreground">
                      {t("templates.detail.repository", {
                        defaultValue: "仓库",
                      })}
                    </p>
                    <div className="flex items-center gap-1">
                      <p className="text-sm font-medium truncate">
                        {component.repoOwner}/{component.repoName}
                        {component.repoBranch &&
                          component.repoBranch !== "main" &&
                          ` (${component.repoBranch})`}
                      </p>
                      {component.readmeUrl && (
                        <Button
                          variant="ghost"
                          size="sm"
                          onClick={handleOpenGithub}
                          className="h-6 w-6 p-0"
                        >
                          <ExternalLink className="h-3.5 w-3.5" />
                        </Button>
                      )}
                    </div>
                  </div>
                </div>

                {/* 更新时间 */}
                {component.updatedAt && (
                  <div className="flex items-center gap-3 p-3 rounded-lg bg-muted/30">
                    <div className="flex items-center justify-center w-10 h-10 rounded-lg bg-primary/10">
                      <Clock className="h-5 w-5 text-primary" />
                    </div>
                    <div className="flex-1 min-w-0">
                      <p className="text-xs text-muted-foreground">
                        {t("templates.detail.updatedAt", {
                          defaultValue: "更新时间",
                        })}
                      </p>
                      <p className="text-sm font-medium truncate">
                        {new Date(component.updatedAt).toLocaleString()}
                      </p>
                    </div>
                  </div>
                )}
              </div>

              {/* 路径 */}
              <div className="mt-3 p-3 rounded-lg bg-muted/30">
                <p className="text-xs text-muted-foreground mb-1">
                  {t("templates.detail.path", { defaultValue: "路径" })}
                </p>
                <p className="text-sm font-mono text-muted-foreground break-all">
                  {component.path}
                </p>
              </div>
            </TabsContent>

            <TabsContent
              value="preview"
              className="flex-1 overflow-y-auto mt-4"
            >
              {previewLoading ? (
                <div className="flex items-center justify-center h-32">
                  <Loader2 className="h-6 w-6 animate-spin text-muted-foreground" />
                </div>
              ) : preview ? (
                <pre className="text-xs bg-muted/50 rounded-lg p-4 overflow-auto font-mono whitespace-pre-wrap break-words">
                  {preview}
                </pre>
              ) : (
                <p className="text-sm text-muted-foreground text-center py-8">
                  {t("templates.detail.noPreview", {
                    defaultValue: "无法预览内容",
                  })}
                </p>
              )}
            </TabsContent>
          </Tabs>
        </div>

        <DialogFooter className="flex-row gap-2 justify-end border-t pt-4">
          <Button variant="outline" onClick={onClose}>
            {t("common.close", { defaultValue: "关闭" })}
          </Button>
          {component.installed ? (
            <Button
              variant="destructive"
              onClick={handleUninstall}
              disabled={loading}
            >
              {loading ? (
                <Loader2 className="h-4 w-4 mr-2 animate-spin" />
              ) : (
                <Trash2 className="h-4 w-4 mr-2" />
              )}
              {loading
                ? t("templates.uninstalling", { defaultValue: "卸载中..." })
                : t("templates.uninstall", { defaultValue: "卸载" })}
            </Button>
          ) : (
            <Button onClick={handleInstall} disabled={loading}>
              {loading ? (
                <Loader2 className="h-4 w-4 mr-2 animate-spin" />
              ) : (
                <Download className="h-4 w-4 mr-2" />
              )}
              {loading
                ? t("templates.installing", { defaultValue: "安装中..." })
                : t("templates.install", { defaultValue: "安装" })}
            </Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
