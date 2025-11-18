import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Plus, Settings, Edit3 } from "lucide-react";
import type { Provider } from "@/types";
import type { EnvConflict } from "@/types/env";
import { useProvidersQuery } from "@/lib/query";
import {
  providersApi,
  settingsApi,
  type AppId,
  type ProviderSwitchEvent,
} from "@/lib/api";
import { checkAllEnvConflicts, checkEnvConflicts } from "@/lib/api/env";
import { useProviderActions } from "@/hooks/useProviderActions";
import { extractErrorMessage } from "@/utils/errorUtils";
import { AppSwitcher } from "@/components/AppSwitcher";
import { ProviderList } from "@/components/providers/ProviderList";
import { AddProviderDialog } from "@/components/providers/AddProviderDialog";
import { EditProviderDialog } from "@/components/providers/EditProviderDialog";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { SettingsDialog } from "@/components/settings/SettingsDialog";
import { UpdateBadge } from "@/components/UpdateBadge";
import { EnvWarningBanner } from "@/components/env/EnvWarningBanner";
import UsageScriptModal from "@/components/UsageScriptModal";
import UnifiedMcpPanel from "@/components/mcp/UnifiedMcpPanel";
import PromptPanel from "@/components/prompts/PromptPanel";
import { SkillsPage } from "@/components/skills/SkillsPage";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { VisuallyHidden } from "@radix-ui/react-visually-hidden";

function App() {
  const { t } = useTranslation();

  const [activeApp, setActiveApp] = useState<AppId>("claude");
  const [isEditMode, setIsEditMode] = useState(false);
  const [isSettingsOpen, setIsSettingsOpen] = useState(false);
  const [isAddOpen, setIsAddOpen] = useState(false);
  const [isMcpOpen, setIsMcpOpen] = useState(false);
  const [isPromptOpen, setIsPromptOpen] = useState(false);
  const [isSkillsOpen, setIsSkillsOpen] = useState(false);
  const [editingProvider, setEditingProvider] = useState<Provider | null>(null);
  const [usageProvider, setUsageProvider] = useState<Provider | null>(null);
  const [confirmDelete, setConfirmDelete] = useState<Provider | null>(null);
  const [envConflicts, setEnvConflicts] = useState<EnvConflict[]>([]);
  const [showEnvBanner, setShowEnvBanner] = useState(false);

  const { data, isLoading, refetch } = useProvidersQuery(activeApp);
  const providers = useMemo(() => data?.providers ?? {}, [data]);
  const currentProviderId = data?.currentProviderId ?? "";

  // 🎯 使用 useProviderActions Hook 统一管理所有 Provider 操作
  const {
    addProvider,
    updateProvider,
    switchProvider,
    deleteProvider,
    saveUsageScript,
  } = useProviderActions(activeApp);

  // 监听来自托盘菜单的切换事件
  useEffect(() => {
    let unsubscribe: (() => void) | undefined;

    const setupListener = async () => {
      try {
        unsubscribe = await providersApi.onSwitched(
          async (event: ProviderSwitchEvent) => {
            if (event.appType === activeApp) {
              await refetch();
            }
          },
        );
      } catch (error) {
        console.error("[App] Failed to subscribe provider switch event", error);
      }
    };

    setupListener();
    return () => {
      unsubscribe?.();
    };
  }, [activeApp, refetch]);

  // 应用启动时检测所有应用的环境变量冲突
  useEffect(() => {
    const checkEnvOnStartup = async () => {
      try {
        const allConflicts = await checkAllEnvConflicts();
        const flatConflicts = Object.values(allConflicts).flat();

        if (flatConflicts.length > 0) {
          setEnvConflicts(flatConflicts);
          setShowEnvBanner(true);
        }
      } catch (error) {
        console.error("[App] Failed to check environment conflicts on startup:", error);
      }
    };

    checkEnvOnStartup();
  }, []);

  // 切换应用时检测当前应用的环境变量冲突
  useEffect(() => {
    const checkEnvOnSwitch = async () => {
      try {
        const conflicts = await checkEnvConflicts(activeApp);

        if (conflicts.length > 0) {
          // 合并新检测到的冲突
          setEnvConflicts((prev) => {
            const existingKeys = new Set(
              prev.map((c) => `${c.varName}:${c.sourcePath}`)
            );
            const newConflicts = conflicts.filter(
              (c) => !existingKeys.has(`${c.varName}:${c.sourcePath}`)
            );
            return [...prev, ...newConflicts];
          });
          setShowEnvBanner(true);
        }
      } catch (error) {
        console.error("[App] Failed to check environment conflicts on app switch:", error);
      }
    };

    checkEnvOnSwitch();
  }, [activeApp]);

  // 打开网站链接
  const handleOpenWebsite = async (url: string) => {
    try {
      await settingsApi.openExternal(url);
    } catch (error) {
      const detail =
        extractErrorMessage(error) ||
        t("notifications.openLinkFailed", {
          defaultValue: "链接打开失败",
        });
      toast.error(detail);
    }
  };

  // 编辑供应商
  const handleEditProvider = async (provider: Provider) => {
    await updateProvider(provider);
    setEditingProvider(null);
  };

  // 确认删除供应商
  const handleConfirmDelete = async () => {
    if (!confirmDelete) return;
    await deleteProvider(confirmDelete.id);
    setConfirmDelete(null);
  };

  // 复制供应商
  const handleDuplicateProvider = async (provider: Provider) => {
    // 1️⃣ 计算新的 sortIndex：如果原供应商有 sortIndex，则复制它
    const newSortIndex =
      provider.sortIndex !== undefined ? provider.sortIndex + 1 : undefined;

    const duplicatedProvider: Omit<Provider, "id" | "createdAt"> = {
      name: `${provider.name} copy`,
      settingsConfig: JSON.parse(JSON.stringify(provider.settingsConfig)), // 深拷贝
      websiteUrl: provider.websiteUrl,
      category: provider.category,
      sortIndex: newSortIndex, // 复制原 sortIndex + 1
      meta: provider.meta
        ? JSON.parse(JSON.stringify(provider.meta))
        : undefined, // 深拷贝
    };

    // 2️⃣ 如果原供应商有 sortIndex，需要将后续所有供应商的 sortIndex +1
    if (provider.sortIndex !== undefined) {
      const updates = Object.values(providers)
        .filter(
          (p) =>
            p.sortIndex !== undefined &&
            p.sortIndex >= newSortIndex! &&
            p.id !== provider.id,
        )
        .map((p) => ({
          id: p.id,
          sortIndex: p.sortIndex! + 1,
        }));

      // 先更新现有供应商的 sortIndex，为新供应商腾出位置
      if (updates.length > 0) {
        try {
          await providersApi.updateSortOrder(updates, activeApp);
        } catch (error) {
          console.error("[App] Failed to update sort order", error);
          toast.error(
            t("provider.sortUpdateFailed", {
              defaultValue: "排序更新失败",
            }),
          );
          return; // 如果排序更新失败，不继续添加
        }
      }
    }

    // 3️⃣ 添加复制的供应商
    await addProvider(duplicatedProvider);
  };

  // 导入配置成功后刷新
  const handleImportSuccess = async () => {
    await refetch();
    try {
      await providersApi.updateTrayMenu();
    } catch (error) {
      console.error("[App] Failed to refresh tray menu", error);
    }
  };

  return (
    <div className="flex h-screen flex-col bg-gray-50 dark:bg-gray-950">
      {/* 环境变量警告横幅 */}
      {showEnvBanner && envConflicts.length > 0 && (
        <EnvWarningBanner
          conflicts={envConflicts}
          onDismiss={() => setShowEnvBanner(false)}
          onDeleted={async () => {
            // 删除后重新检测
            try {
              const allConflicts = await checkAllEnvConflicts();
              const flatConflicts = Object.values(allConflicts).flat();
              setEnvConflicts(flatConflicts);
              if (flatConflicts.length === 0) {
                setShowEnvBanner(false);
              }
            } catch (error) {
              console.error("[App] Failed to re-check conflicts after deletion:", error);
            }
          }}
        />
      )}

      <header className="flex-shrink-0 border-b border-gray-200 bg-white px-6 py-4 dark:border-gray-800 dark:bg-gray-900">
        <div className="flex flex-wrap items-center justify-between gap-2">
          <div className="flex items-center gap-1">
            <a
              href="https://github.com/farion1231/cc-switch"
              target="_blank"
              rel="noreferrer"
              className="text-xl font-semibold text-blue-500 transition-colors hover:text-blue-600 dark:text-blue-400 dark:hover:text-blue-300"
            >
              CC Switch
            </a>
            <Button
              variant="ghost"
              size="icon"
              onClick={() => setIsSettingsOpen(true)}
              title={t("common.settings")}
              className="ml-2"
            >
              <Settings className="h-4 w-4" />
            </Button>
            <Button
              variant="ghost"
              size="icon"
              onClick={() => setIsEditMode(!isEditMode)}
              title={t(
                isEditMode ? "header.exitEditMode" : "header.enterEditMode",
              )}
              className={
                isEditMode
                  ? "text-blue-500 hover:text-blue-600 dark:text-blue-400 dark:hover:text-blue-300"
                  : ""
              }
            >
              <Edit3 className="h-4 w-4" />
            </Button>
            <UpdateBadge onClick={() => setIsSettingsOpen(true)} />
          </div>

          <div className="flex flex-wrap items-center gap-2">
            <AppSwitcher activeApp={activeApp} onSwitch={setActiveApp} />
            <Button
              variant="mcp"
              onClick={() => setIsPromptOpen(true)}
              className="min-w-[80px]"
            >
              {t("prompts.manage")}
            </Button>
            <Button
              variant="mcp"
              onClick={() => setIsMcpOpen(true)}
              className="min-w-[80px]"
            >
              MCP
            </Button>
            <Button
              variant="mcp"
              onClick={() => setIsSkillsOpen(true)}
              className="min-w-[80px]"
            >
              {t("skills.manage")}
            </Button>
            <Button onClick={() => setIsAddOpen(true)}>
              <Plus className="h-4 w-4" />
              {t("header.addProvider")}
            </Button>
          </div>
        </div>
      </header>

      <main className="flex-1 overflow-y-scroll">
        <div className="mx-auto max-w-4xl px-6 py-6">
          <ProviderList
            providers={providers}
            currentProviderId={currentProviderId}
            appId={activeApp}
            isLoading={isLoading}
            isEditMode={isEditMode}
            onSwitch={switchProvider}
            onEdit={setEditingProvider}
            onDelete={setConfirmDelete}
            onDuplicate={handleDuplicateProvider}
            onConfigureUsage={setUsageProvider}
            onOpenWebsite={handleOpenWebsite}
            onCreate={() => setIsAddOpen(true)}
          />
        </div>
      </main>

      <AddProviderDialog
        open={isAddOpen}
        onOpenChange={setIsAddOpen}
        appId={activeApp}
        onSubmit={addProvider}
      />

      <EditProviderDialog
        open={Boolean(editingProvider)}
        provider={editingProvider}
        onOpenChange={(open) => {
          if (!open) {
            setEditingProvider(null);
          }
        }}
        onSubmit={handleEditProvider}
        appId={activeApp}
      />

      {usageProvider && (
        <UsageScriptModal
          provider={usageProvider}
          appId={activeApp}
          isOpen={Boolean(usageProvider)}
          onClose={() => setUsageProvider(null)}
          onSave={(script) => {
            void saveUsageScript(usageProvider, script);
          }}
        />
      )}

      <ConfirmDialog
        isOpen={Boolean(confirmDelete)}
        title={t("confirm.deleteProvider")}
        message={
          confirmDelete
            ? t("confirm.deleteProviderMessage", {
                name: confirmDelete.name,
              })
            : ""
        }
        onConfirm={() => void handleConfirmDelete()}
        onCancel={() => setConfirmDelete(null)}
      />

      <SettingsDialog
        open={isSettingsOpen}
        onOpenChange={setIsSettingsOpen}
        onImportSuccess={handleImportSuccess}
      />

      <PromptPanel
        open={isPromptOpen}
        onOpenChange={setIsPromptOpen}
        appId={activeApp}
      />

      <UnifiedMcpPanel open={isMcpOpen} onOpenChange={setIsMcpOpen} />

      <Dialog open={isSkillsOpen} onOpenChange={setIsSkillsOpen}>
        <DialogContent className="max-w-4xl max-h-[85vh] min-h-[600px] flex flex-col p-0">
          <DialogHeader className="sr-only">
            <VisuallyHidden>
              <DialogTitle>{t("skills.title")}</DialogTitle>
            </VisuallyHidden>
          </DialogHeader>
          <SkillsPage onClose={() => setIsSkillsOpen(false)} />
        </DialogContent>
      </Dialog>
    </div>
  );
}

export default App;
