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
import { Download, Loader2, Trash2 } from "lucide-react";
import type { MarketplaceBundle } from "@/types/template";
import type { AppType } from "@/lib/api/config";

// 组件类型图标映射
const componentTypeIcons: Record<string, string> = {
  agent: "🤖",
  command: "⚡",
  mcp: "🔌",
  setting: "⚙️",
  hook: "🪝",
  skill: "💡",
};

interface BundleInstallStatus {
  installed: boolean;
  installedIds: number[];
  totalCount: number;
  installedCount: number;
}

interface BundleDetailProps {
  bundle: MarketplaceBundle;
  status?: BundleInstallStatus;
  selectedApp: AppType;
  onClose: () => void;
  onInstall: () => void;
  onUninstall: () => void;
  installing: boolean;
  uninstalling: boolean;
}

export function BundleDetail({
  bundle,
  status,
  onClose,
  onInstall,
  onUninstall,
  installing,
  uninstalling,
}: BundleDetailProps) {
  const { t } = useTranslation();

  // 按类型分组组件
  const componentsByType = bundle.components.reduce(
    (acc, comp) => {
      const type = comp.componentType;
      if (!acc[type]) acc[type] = [];
      acc[type].push(comp);
      return acc;
    },
    {} as Record<string, typeof bundle.components>,
  );

  return (
    <Dialog open={true} onOpenChange={onClose}>
      <DialogContent className="max-w-2xl max-h-[80vh] overflow-hidden flex flex-col">
        <DialogHeader>
          <div className="flex items-start justify-between gap-4">
            <div className="flex-1 min-w-0">
              <div className="flex items-center gap-2 mb-2">
                <span className="text-3xl">📦</span>
                {status?.installed && (
                  <Badge
                    variant="default"
                    className="bg-green-600/90 hover:bg-green-600 dark:bg-green-700/90 dark:hover:bg-green-700 text-white border-0"
                  >
                    {t("templates.installed", { defaultValue: "已安装" })}
                  </Badge>
                )}
              </div>
              <DialogTitle className="text-2xl">{bundle.name}</DialogTitle>
              <DialogDescription className="text-sm mt-2">
                {bundle.description ||
                  t("templates.noDescription", { defaultValue: "暂无描述" })}
              </DialogDescription>
            </div>
          </div>
        </DialogHeader>

        {/* 组件列表 */}
        <div className="flex-1 overflow-y-auto space-y-6 py-4 px-1">
          {Object.entries(componentsByType).map(([type, components]) => (
            <div key={type}>
              <div className="flex items-center justify-center gap-2 mb-3">
                <span className="text-xl">
                  {componentTypeIcons[type] || "📦"}
                </span>
                <h3 className="font-medium text-foreground">
                  {t(`templates.type.${type}`, { defaultValue: type })}
                </h3>
                <Badge variant="secondary" className="text-xs">
                  {components.length}
                </Badge>
              </div>
              <div className="grid grid-cols-1 sm:grid-cols-2 gap-3 px-2">
                {components.map((comp, idx) => (
                  <div
                    key={`${comp.name}-${idx}`}
                    className="flex items-center gap-3 p-3 rounded-lg bg-muted/30"
                  >
                    <span className="text-lg">
                      {componentTypeIcons[type] || "📦"}
                    </span>
                    <div className="flex-1 min-w-0">
                      <p className="font-medium text-sm truncate">
                        {comp.name}
                      </p>
                      <p className="text-xs text-muted-foreground truncate">
                        {comp.path}
                      </p>
                    </div>
                  </div>
                ))}
              </div>
            </div>
          ))}
        </div>

        <DialogFooter className="flex-row gap-2 justify-end border-t pt-4">
          <Button variant="outline" onClick={onClose}>
            {t("common.close", { defaultValue: "关闭" })}
          </Button>
          {status && status.installedCount > 0 && (
            <Button
              variant="destructive"
              onClick={onUninstall}
              disabled={uninstalling}
            >
              {uninstalling ? (
                <Loader2 className="h-4 w-4 mr-2 animate-spin" />
              ) : (
                <Trash2 className="h-4 w-4 mr-2" />
              )}
              {t("templates.bundle.uninstall", { defaultValue: "卸载" })}
            </Button>
          )}
          {!status?.installed && (
            <Button onClick={onInstall} disabled={installing}>
              {installing ? (
                <Loader2 className="h-4 w-4 mr-2 animate-spin" />
              ) : (
                <Download className="h-4 w-4 mr-2" />
              )}
              {t("templates.bundle.install", { defaultValue: "安装组合" })}
            </Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
