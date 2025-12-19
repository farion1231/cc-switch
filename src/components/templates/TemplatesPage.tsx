import { useState, useMemo } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { RefreshCw, Search, Settings, Loader2 } from "lucide-react";
import { toast } from "sonner";
import { ComponentCard } from "./ComponentCard";
import { ComponentDetail } from "./ComponentDetail";
import { CategoryFilter } from "./CategoryFilter";
import { RepoManager } from "./RepoManager";
import { BundleList } from "./BundleList";
import {
  useTemplateComponents,
  useComponentCategories,
  useInstallTemplateComponent,
  useUninstallTemplateComponent,
  useRefreshTemplateIndex,
} from "@/lib/query/template";
import type { ComponentType, TemplateComponent } from "@/types/template";
import type { AppType } from "@/lib/api/config";

interface TemplatesPageProps {
  activeApp: AppType;
}

export function TemplatesPage({ activeApp }: TemplatesPageProps) {
  const { t } = useTranslation();
  const [selectedType, setSelectedType] = useState<ComponentType | "bundle">(
    "bundle",
  );
  const [selectedCategory, setSelectedCategory] = useState<string | undefined>(
    undefined,
  );
  const [searchQuery, setSearchQuery] = useState("");
  const [repoManagerOpen, setRepoManagerOpen] = useState(false);
  const [detailComponent, setDetailComponent] = useState<
    TemplateComponent | undefined
  >(undefined);

  // Queries
  const {
    data: componentsData,
    isLoading: componentsLoading,
    refetch: refetchComponents,
  } = useTemplateComponents({
    componentType: selectedType === "bundle" ? undefined : selectedType,
    category: selectedCategory,
    search: searchQuery || undefined,
    appType: activeApp,
  });

  const { data: categories = [] } = useComponentCategories(
    selectedType === "bundle" ? undefined : selectedType,
  );

  // Mutations
  const installMutation = useInstallTemplateComponent();
  const uninstallMutation = useUninstallTemplateComponent();
  const refreshMutation = useRefreshTemplateIndex();

  const handleInstall = async (id: number, name: string) => {
    try {
      await installMutation.mutateAsync({ id, appType: activeApp });
      toast.success(
        t("templates.installSuccess", { name, defaultValue: `已安装 ${name}` }),
      );
    } catch (error) {
      const errorMessage =
        error instanceof Error ? error.message : String(error);
      toast.error(
        t("templates.installFailed", {
          name,
          defaultValue: `安装 ${name} 失败`,
        }),
        {
          description: errorMessage,
          duration: 8000,
        },
      );
      console.error("Install component failed:", error);
    }
  };

  const handleUninstall = async (id: number, name: string) => {
    try {
      await uninstallMutation.mutateAsync({ id, appType: activeApp });
      toast.success(
        t("templates.uninstallSuccess", {
          name,
          defaultValue: `已卸载 ${name}`,
        }),
      );
    } catch (error) {
      const errorMessage =
        error instanceof Error ? error.message : String(error);
      toast.error(
        t("templates.uninstallFailed", {
          name,
          defaultValue: `卸载 ${name} 失败`,
        }),
        {
          description: errorMessage,
          duration: 8000,
        },
      );
      console.error("Uninstall component failed:", error);
    }
  };

  const handleRefresh = async () => {
    try {
      await refreshMutation.mutateAsync();
      await refetchComponents();
      toast.success(
        t("templates.refreshSuccess", { defaultValue: "刷新成功" }),
      );
    } catch (error) {
      const errorMessage =
        error instanceof Error ? error.message : String(error);
      toast.error(t("templates.refreshFailed", { defaultValue: "刷新失败" }), {
        description: errorMessage,
        duration: 8000,
      });
      console.error("Refresh index failed:", error);
    }
  };

  const components = componentsData?.items || [];

  // 过滤组件
  const filteredComponents = useMemo(() => {
    if (!searchQuery.trim()) return components;

    const query = searchQuery.toLowerCase();
    return components.filter((component) => {
      const name = component.name?.toLowerCase() || "";
      const description = component.description?.toLowerCase() || "";
      const category = component.category?.toLowerCase() || "";

      return (
        name.includes(query) ||
        description.includes(query) ||
        category.includes(query)
      );
    });
  }, [components, searchQuery]);

  const componentTypeOptions: {
    value: ComponentType | "bundle";
    icon: string;
  }[] = [
    { value: "bundle", icon: "📦" },
    { value: "agent", icon: "🤖" },
    { value: "command", icon: "⚡" },
    { value: "mcp", icon: "🔌" },
    { value: "setting", icon: "⚙️" },
    { value: "hook", icon: "🪝" },
    { value: "skill", icon: "💡" },
  ];

  return (
    <div className="mx-auto max-w-[80rem] px-6 flex h-[calc(100vh-8rem)] overflow-hidden bg-background/50">
      {/* 左侧分类过滤器 */}
      <div className="w-48 shrink-0 mr-6 overflow-y-auto">
        <CategoryFilter
          categories={categories}
          selectedCategory={selectedCategory}
          onSelectCategory={setSelectedCategory}
        />
      </div>

      {/* 右侧主内容区 */}
      <div className="flex-1 flex flex-col overflow-hidden">
        {/* 顶部工具栏 */}
        <div className="mb-6 space-y-4">
          {/* 操作按钮 */}
          <div className="flex items-center justify-end gap-4">
            <div className="flex gap-2">
              <Button
                variant="outline"
                size="sm"
                onClick={handleRefresh}
                disabled={refreshMutation.isPending}
              >
                {refreshMutation.isPending ? (
                  <Loader2 className="h-4 w-4 mr-2 animate-spin" />
                ) : (
                  <RefreshCw className="h-4 w-4 mr-2" />
                )}
                {t("templates.refresh", { defaultValue: "刷新索引" })}
              </Button>
              <Button
                variant="outline"
                size="sm"
                onClick={() => setRepoManagerOpen(true)}
              >
                <Settings className="h-4 w-4 mr-2" />
                {t("templates.manageRepos", { defaultValue: "管理仓库" })}
              </Button>
            </div>
          </div>

          {/* 搜索框 */}
          <div className="relative">
            <Search className="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground" />
            <Input
              type="text"
              placeholder={t("templates.searchPlaceholder", {
                defaultValue: "搜索组件...",
              })}
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              className="pl-9 pr-3"
            />
          </div>
        </div>

        {/* 类型标签页 */}
        <Tabs
          value={selectedType}
          onValueChange={(value) => {
            setSelectedType(value as ComponentType | "bundle");
            setSelectedCategory(undefined);
          }}
          className="flex-1 flex flex-col overflow-hidden"
        >
          <TabsList className="w-full justify-start mb-4">
            {componentTypeOptions.map((option) => (
              <TabsTrigger key={option.value} value={option.value}>
                <span className="mr-1.5">{option.icon}</span>
                {t(`templates.type.${option.value}`, {
                  defaultValue: option.value,
                })}
              </TabsTrigger>
            ))}
          </TabsList>

          {/* 组合标签页 */}
          <TabsContent value="bundle" className="flex-1 overflow-y-auto mt-0">
            <div className="py-4">
              <BundleList selectedApp={activeApp} />
            </div>
          </TabsContent>

          {/* 组件类型标签页 */}
          {componentTypeOptions
            .filter((opt) => opt.value !== "bundle")
            .map((option) => (
              <TabsContent
                key={option.value}
                value={option.value}
                className="flex-1 overflow-y-auto mt-0"
              >
                <div className="py-4">
                  {componentsLoading ? (
                    <div className="flex items-center justify-center h-64">
                      <RefreshCw className="h-8 w-8 animate-spin text-muted-foreground" />
                    </div>
                  ) : filteredComponents.length === 0 ? (
                    <div className="flex flex-col items-center justify-center h-64 text-center">
                      <p className="text-lg font-medium text-gray-900 dark:text-gray-100">
                        {t("templates.empty", { defaultValue: "暂无组件" })}
                      </p>
                      <p className="mt-2 text-sm text-gray-500 dark:text-gray-400">
                        {t("templates.emptyDescription", {
                          defaultValue: "请添加模板仓库来获取组件",
                        })}
                      </p>
                    </div>
                  ) : (
                    <>
                      {searchQuery && (
                        <p className="mb-4 text-sm text-muted-foreground">
                          {t("templates.count", {
                            count: filteredComponents.length,
                            defaultValue: `找到 ${filteredComponents.length} 个组件`,
                          })}
                        </p>
                      )}
                      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
                        {filteredComponents.map((component) => (
                          <ComponentCard
                            key={
                              component.id ??
                              `${component.repoId}-${component.path}`
                            }
                            component={component}
                            onInstall={async () => {
                              if (component.id !== null) {
                                await handleInstall(
                                  component.id,
                                  component.name,
                                );
                              }
                            }}
                            onUninstall={async () => {
                              if (component.id !== null) {
                                await handleUninstall(
                                  component.id,
                                  component.name,
                                );
                              }
                            }}
                            onViewDetail={() => setDetailComponent(component)}
                          />
                        ))}
                      </div>
                    </>
                  )}
                </div>
              </TabsContent>
            ))}
        </Tabs>
      </div>

      {/* 仓库管理弹窗 */}
      {repoManagerOpen && (
        <RepoManager onClose={() => setRepoManagerOpen(false)} />
      )}

      {/* 组件详情弹窗 */}
      {detailComponent && detailComponent.id !== null && (
        <ComponentDetail
          componentId={detailComponent.id}
          selectedApp={activeApp}
          onClose={() => setDetailComponent(undefined)}
          onInstall={handleInstall}
          onUninstall={handleUninstall}
        />
      )}
    </div>
  );
}
