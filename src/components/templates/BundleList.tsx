import { useState, useEffect, useCallback } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Download, Loader2, Package, Trash2, FileText } from "lucide-react";
import { toast } from "sonner";
import {
  useMarketplaceBundles,
  useBatchInstallComponents,
} from "@/lib/query/template";
import { templateApi } from "@/lib/api/template";
import { BundleDetail } from "./BundleDetail";
import type { MarketplaceBundle, ComponentType } from "@/types/template";
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

interface BundleListProps {
  selectedApp: AppType;
}

// 统计组件类型数量
function getComponentTypeCounts(components: MarketplaceBundle["components"]) {
  const counts: Record<string, number> = {};
  for (const comp of components) {
    const type = comp.componentType;
    counts[type] = (counts[type] || 0) + 1;
  }
  return counts;
}

export function BundleList({ selectedApp }: BundleListProps) {
  const { t } = useTranslation();
  const [installingBundle, setInstallingBundle] = useState<string | null>(null);
  const [uninstallingBundle, setUninstallingBundle] = useState<string | null>(
    null,
  );
  const [bundleStatuses, setBundleStatuses] = useState<
    Record<string, BundleInstallStatus>
  >({});
  const [detailBundle, setDetailBundle] = useState<MarketplaceBundle | null>(
    null,
  );

  const { data: bundles = [], isLoading } = useMarketplaceBundles();
  const batchInstallMutation = useBatchInstallComponents();

  // 检查组合安装状态
  const checkBundleStatus = useCallback(
    async (bundle: MarketplaceBundle): Promise<BundleInstallStatus> => {
      const componentsByType = bundle.components.reduce(
        (acc, comp) => {
          const type = comp.componentType;
          if (!acc[type]) acc[type] = [];
          acc[type].push(comp.name.toLowerCase());
          return acc;
        },
        {} as Record<string, string[]>,
      );

      const installedIds: number[] = [];
      let totalMatched = 0;

      for (const [componentType, names] of Object.entries(componentsByType)) {
        const componentsData = await templateApi.listTemplateComponents({
          componentType: componentType as ComponentType,
          pageSize: 1000,
          appType: selectedApp,
        });

        for (const comp of componentsData.items) {
          if (names.includes(comp.name.toLowerCase())) {
            totalMatched++;
            if (comp.installed && comp.id !== null) {
              installedIds.push(comp.id);
            }
          }
        }
      }

      return {
        installed:
          installedIds.length > 0 && installedIds.length === totalMatched,
        installedIds,
        totalCount: totalMatched,
        installedCount: installedIds.length,
      };
    },
    [selectedApp],
  );

  // 加载所有组合的安装状态
  useEffect(() => {
    const loadStatuses = async () => {
      const statuses: Record<string, BundleInstallStatus> = {};
      for (const bundle of bundles) {
        statuses[bundle.id] = await checkBundleStatus(bundle);
      }
      setBundleStatuses(statuses);
    };
    if (bundles.length > 0) {
      loadStatuses();
    }
  }, [bundles, checkBundleStatus]);

  const handleInstallBundle = async (bundle: MarketplaceBundle) => {
    setInstallingBundle(bundle.id);
    try {
      // 按组件类型分组
      const componentsByType = bundle.components.reduce(
        (acc, comp) => {
          const type = comp.componentType;
          if (!acc[type]) acc[type] = [];
          acc[type].push(comp.name.toLowerCase());
          return acc;
        },
        {} as Record<string, string[]>,
      );

      // 收集所有匹配的组件 ID
      const matchedIds: number[] = [];

      for (const [componentType, names] of Object.entries(componentsByType)) {
        const componentsData = await templateApi.listTemplateComponents({
          componentType: componentType as ComponentType,
          pageSize: 1000,
        });

        const ids = componentsData.items
          .filter((c) => names.includes(c.name.toLowerCase()))
          .map((c) => c.id)
          .filter((id): id is number => id !== null);

        matchedIds.push(...ids);
      }

      if (matchedIds.length === 0) {
        toast.warning(
          t("templates.bundle.noMatch", {
            defaultValue: "未找到匹配的组件",
          }),
        );
        return;
      }

      const result = await batchInstallMutation.mutateAsync({
        ids: matchedIds,
        appType: selectedApp,
      });

      toast.success(
        t("templates.bundle.installSuccess", {
          count: result.success.length,
          defaultValue: `已安装 ${result.success.length} 个组件`,
        }),
      );

      if (result.failed.length > 0) {
        toast.warning(
          t("templates.bundle.partialFail", {
            count: result.failed.length,
            defaultValue: `${result.failed.length} 个组件安装失败`,
          }),
        );
      }

      // 刷新安装状态
      const newStatus = await checkBundleStatus(bundle);
      setBundleStatuses((prev) => ({ ...prev, [bundle.id]: newStatus }));
    } catch (error) {
      const errorMessage =
        error instanceof Error ? error.message : String(error);
      toast.error(
        t("templates.bundle.installFailed", { defaultValue: "安装组合失败" }),
        { description: errorMessage, duration: 8000 },
      );
    } finally {
      setInstallingBundle(null);
    }
  };

  const handleUninstallBundle = async (bundle: MarketplaceBundle) => {
    const status = bundleStatuses[bundle.id];
    if (!status || status.installedIds.length === 0) return;

    setUninstallingBundle(bundle.id);
    try {
      let successCount = 0;
      let failCount = 0;

      for (const id of status.installedIds) {
        try {
          await templateApi.uninstallTemplateComponent(id, selectedApp);
          successCount++;
        } catch {
          failCount++;
        }
      }

      if (successCount > 0) {
        toast.success(
          t("templates.bundle.uninstallSuccess", {
            count: successCount,
            defaultValue: `已卸载 ${successCount} 个组件`,
          }),
        );
      }

      if (failCount > 0) {
        toast.warning(
          t("templates.bundle.uninstallPartialFail", {
            count: failCount,
            defaultValue: `${failCount} 个组件卸载失败`,
          }),
        );
      }

      // 刷新安装状态
      const newStatus = await checkBundleStatus(bundle);
      setBundleStatuses((prev) => ({ ...prev, [bundle.id]: newStatus }));
    } catch (error) {
      const errorMessage =
        error instanceof Error ? error.message : String(error);
      toast.error(
        t("templates.bundle.uninstallFailed", { defaultValue: "卸载组合失败" }),
        { description: errorMessage, duration: 8000 },
      );
    } finally {
      setUninstallingBundle(null);
    }
  };

  if (isLoading) {
    return (
      <div className="flex items-center justify-center h-64">
        <Loader2 className="h-8 w-8 animate-spin text-muted-foreground" />
      </div>
    );
  }

  if (bundles.length === 0) {
    return (
      <div className="flex flex-col items-center justify-center h-64 text-center">
        <Package className="h-12 w-12 text-muted-foreground mb-4" />
        <p className="text-lg font-medium text-foreground">
          {t("templates.bundle.empty", { defaultValue: "暂无组合" })}
        </p>
        <p className="mt-2 text-sm text-muted-foreground">
          {t("templates.bundle.emptyDescription", {
            defaultValue: "请添加包含 components.json 的模板仓库",
          })}
        </p>
      </div>
    );
  }

  return (
    <>
      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
        {bundles.map((bundle) => {
          const typeCounts = getComponentTypeCounts(bundle.components);
          const status = bundleStatuses[bundle.id];

          return (
            <div
              key={bundle.id}
              className="glass-card rounded-xl p-4 flex flex-col h-full transition-all duration-300 hover:scale-[1.01] hover:shadow-lg group relative overflow-hidden cursor-pointer"
              onClick={() => setDetailBundle(bundle)}
            >
              <div className="absolute inset-0 bg-gradient-to-br from-primary/5 to-transparent opacity-0 group-hover:opacity-100 transition-opacity duration-500 pointer-events-none" />

              {/* 头部 */}
              <div className="flex items-start justify-between gap-2 mb-3">
                <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-2 mb-1.5">
                    <span className="text-2xl">📦</span>
                  </div>
                  <h3 className="font-semibold text-foreground truncate">
                    {bundle.name}
                  </h3>
                </div>
                {status?.installed && (
                  <Badge
                    variant="default"
                    className="shrink-0 bg-green-600/90 hover:bg-green-600 dark:bg-green-700/90 dark:hover:bg-green-700 text-white border-0"
                  >
                    {t("templates.installed", { defaultValue: "已安装" })}
                  </Badge>
                )}
              </div>

              {/* 描述 */}
              <p className="text-sm text-muted-foreground/90 line-clamp-2 leading-relaxed mb-3 flex-1">
                {bundle.description ||
                  t("templates.noDescription", { defaultValue: "暂无描述" })}
              </p>

              {/* 组件类型统计 */}
              <div className="flex flex-wrap gap-1.5 mb-3">
                {Object.entries(typeCounts).map(([type, count]) => (
                  <Badge key={type} variant="secondary" className="text-xs">
                    <span className="mr-1">
                      {componentTypeIcons[type] || "📦"}
                    </span>
                    {type} {count}
                  </Badge>
                ))}
              </div>

              {/* 底部操作栏 */}
              <div className="flex gap-2 pt-3 border-t border-border/50 relative z-10">
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={(e) => {
                    e.stopPropagation();
                    setDetailBundle(bundle);
                  }}
                  className="flex-1"
                >
                  <FileText className="h-3.5 w-3.5 mr-1.5" />
                  {t("templates.viewDetail", { defaultValue: "查看详情" })}
                </Button>

                {status && status.installedCount > 0 && (
                  <Button
                    size="sm"
                    variant="outline"
                    onClick={(e) => {
                      e.stopPropagation();
                      handleUninstallBundle(bundle);
                    }}
                    disabled={uninstallingBundle === bundle.id}
                    className="flex-1 border-red-300 text-red-500 hover:bg-red-50 hover:text-red-600 dark:border-red-500/50 dark:text-red-400 dark:hover:bg-red-900/30 dark:hover:text-red-300"
                  >
                    {uninstallingBundle === bundle.id ? (
                      <Loader2 className="h-3.5 w-3.5 mr-1.5 animate-spin" />
                    ) : (
                      <Trash2 className="h-3.5 w-3.5 mr-1.5" />
                    )}
                    {t("templates.bundle.uninstall", { defaultValue: "卸载" })}
                  </Button>
                )}
                {!status?.installed && (
                  <Button
                    variant="mcp"
                    size="sm"
                    onClick={(e) => {
                      e.stopPropagation();
                      handleInstallBundle(bundle);
                    }}
                    disabled={installingBundle === bundle.id}
                    className="flex-1"
                  >
                    {installingBundle === bundle.id ? (
                      <Loader2 className="h-3.5 w-3.5 mr-1.5 animate-spin" />
                    ) : (
                      <Download className="h-3.5 w-3.5 mr-1.5" />
                    )}
                    {t("templates.bundle.install", { defaultValue: "安装" })}
                  </Button>
                )}
              </div>
            </div>
          );
        })}
      </div>

      {/* 详情弹窗 */}
      {detailBundle && (
        <BundleDetail
          bundle={detailBundle}
          status={bundleStatuses[detailBundle.id]}
          selectedApp={selectedApp}
          onClose={() => setDetailBundle(null)}
          onInstall={() => handleInstallBundle(detailBundle)}
          onUninstall={() => handleUninstallBundle(detailBundle)}
          installing={installingBundle === detailBundle.id}
          uninstalling={uninstallingBundle === detailBundle.id}
        />
      )}
    </>
  );
}
