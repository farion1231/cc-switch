import { useState, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { Provider } from "./types";
import { AppType } from "./lib/query";
import { useProvidersQuery, useVSCodeSyncMutation } from "./lib/query";
import ProviderList from "./components/ProviderList";
import { AddProviderDialog } from "./components/AddProviderDialog";
import { EditProviderDialog } from "./components/EditProviderDialog";
import { ConfirmDialog } from "./components/ConfirmDialog";
import { AppSwitcher } from "./components/AppSwitcher";
import { SettingsDialog } from "./components/SettingsDialog";
import {
  Dialog,
  DialogTrigger,
} from "@/components/ui/dialog";
import { UpdateBadge } from "./components/UpdateBadge";
import { Plus, Settings } from "lucide-react";
import { Button } from "@/components/ui/button";
import { ModeToggle } from "./components/mode-toggle";
import { useVSCodeAutoSync } from "./hooks/useVSCodeAutoSync";
import { useQueryClient } from "@tanstack/react-query";
import tauriAPI from "./lib/tauri-api";
import { Toaster } from "./components/ui/sonner";

function App() {
  const { t } = useTranslation();
  const { isAutoSyncEnabled } = useVSCodeAutoSync();
  const queryClient = useQueryClient();
  const [activeApp, setActiveApp] = useState<AppType>("claude");
  const [isAddModalOpen, setIsAddModalOpen] = useState(false);
  const [isSettingsOpen, setIsSettingsOpen] = useState(false);
  const [editingProviderId, setEditingProviderId] = useState<string | null>(
    null
  );
  const [confirmDialog, setConfirmDialog] = useState<{
    isOpen: boolean;
    title: string;
    message: string;
    onConfirm: () => void;
  } | null>(null);

  // Query hooks
  const { data: providersData, isLoading, error } = useProvidersQuery(activeApp);
  const vscodeSyncMutation = useVSCodeSyncMutation(activeApp);

  const providers: Record<string, Provider> = providersData?.providers || Object.create(null);
  const currentProviderId = (providersData?.currentProviderId as string) || "";

  
  
  // 监听托盘切换事件（包括菜单切换）
  useEffect(() => {
    let unlisten: (() => void) | null = null;

    const setupListener = async () => {
      try {
        unlisten = await tauriAPI.onProviderSwitched(async (data) => {
          if (import.meta.env.DEV) {
            console.log(t("console.providerSwitchReceived"), data);
          }

          // 如果当前应用类型匹配，则重新加载数据
          if (data.appType === activeApp) {
            await queryClient.invalidateQueries({ queryKey: ['providers', activeApp] });
          }

          // 若为 Codex 且开启自动同步，则静默同步到 VS Code（覆盖）
          if (data.appType === "codex" && isAutoSyncEnabled) {
            vscodeSyncMutation.mutate(data.providerId);
          }
        });
      } catch (error) {
        console.error(t("console.setupListenerFailed"), error);
      }
    };

    setupListener();

    // 清理监听器
    return () => {
      if (unlisten) {
        unlisten();
      }
    };
  }, [activeApp, isAutoSyncEnabled]);

  
  const handleEditProvider = (providerId: string) => {
    setEditingProviderId(providerId);
  };

  
  
  return (
    <div className="h-screen flex flex-col bg-gray-50 dark:bg-gray-950">
      {/* 顶部导航区域 - 固定高度 */}
      <header className="flex-shrink-0 bg-white border-b border-gray-200 dark:bg-gray-900 dark:border-gray-800 px-6 py-4">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2">
            <a
              href="https://github.com/farion1231/cc-switch"
              target="_blank"
              rel="noopener noreferrer"
              className="text-xl font-semibold text-blue-500 dark:text-blue-400 hover:text-blue-600 dark:hover:text-blue-300 transition-colors"
              title={t("header.viewOnGithub")}
            >
              CC Switch
            </a>
            <ModeToggle />
            <div className="flex items-center gap-2">
              <Dialog open={isSettingsOpen} onOpenChange={setIsSettingsOpen}>
                <DialogTrigger asChild>
                  <Button
                    variant="ghost"
                    size="icon"
                    title={t("common.settings")}
                  >
                    <Settings size={18} />
                  </Button>
                </DialogTrigger>
                <SettingsDialog onOpenChange={setIsSettingsOpen} />
              </Dialog>
              <UpdateBadge onClick={() => {/* Settings dialog can be opened via settings button */}} />
            </div>
          </div>

          <div className="flex items-center gap-4">
            <AppSwitcher activeApp={activeApp} onSwitch={setActiveApp} />

            <Dialog open={isAddModalOpen} onOpenChange={setIsAddModalOpen}>
              <DialogTrigger asChild>
                <Button>
                  <Plus size={16} />
                  {t("header.addProvider")}
                </Button>
              </DialogTrigger>
              <AddProviderDialog appType={activeApp} onOpenChange={setIsAddModalOpen} />
            </Dialog>
          </div>
        </div>
      </header>

      {/* 主内容区域 - 独立滚动 */}
      <main className="flex-1 overflow-y-scroll">
        <div className="pt-3 px-6 pb-6">
          <div className="max-w-4xl mx-auto">
            {/* 通知组件 - 相对于视窗定位 */}
            {isLoading && (
              <div className="flex items-center justify-center py-8">
                <div className="animate-spin rounded-full h-8 w-8 border-b-2 border-blue-500"></div>
                <span className="ml-3 text-gray-600 dark:text-gray-400">
                  {t("common.loading")}
                </span>
              </div>
            )}

            {error && (
              <div className="bg-red-50 dark:bg-red-900/20 border border-red-200 dark:border-red-800 rounded-lg p-4 mb-6">
                <p className="text-red-800 dark:text-red-200">
                  {t("notifications.loadFailed")}
                </p>
              </div>
            )}

            
            {!isLoading && !error && (
              <ProviderList
                providers={providers}
                currentProviderId={currentProviderId}
                onEdit={handleEditProvider}
                appType={activeApp}
              />
            )}
          </div>
        </div>
      </main>

      {editingProviderId && (
        <Dialog open={!!editingProviderId} onOpenChange={(open) => !open && setEditingProviderId(null)}>
          <EditProviderDialog
            appType={activeApp}
            providerId={editingProviderId}
            onOpenChange={(open) => !open && setEditingProviderId(null)}
          />
        </Dialog>
      )}

      {confirmDialog && (
        <ConfirmDialog
          isOpen={confirmDialog.isOpen}
          title={confirmDialog.title}
          message={confirmDialog.message}
          onConfirm={confirmDialog.onConfirm}
          onCancel={() => setConfirmDialog(null)}
        />
      )}

      <Toaster />
    </div>
  );
}

export default App;
