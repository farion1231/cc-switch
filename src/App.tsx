import { useState, useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { Provider } from "./types";
import { AppType } from "./lib/query";
import { useProvidersQuery, useAddProviderMutation, useUpdateProviderMutation, useVSCodeSyncMutation } from "./lib/query";
import ProviderList from "./components/ProviderList";
import AddProviderModal from "./components/AddProviderModal";
import EditProviderModal from "./components/EditProviderModal";
import { ConfirmDialog } from "./components/ConfirmDialog";
import { AppSwitcher } from "./components/AppSwitcher";
import SettingsModal from "./components/SettingsModal";
import { UpdateBadge } from "./components/UpdateBadge";
import { Plus, Settings, Moon, Sun } from "lucide-react";
import { buttonStyles } from "./lib/styles";
import { useDarkMode } from "./hooks/useDarkMode";
import { extractErrorMessage } from "./utils/errorUtils";
import { useVSCodeAutoSync } from "./hooks/useVSCodeAutoSync";
import { useQueryClient } from "@tanstack/react-query";
import tauriAPI from "./lib/tauri-api";

function App() {
  const { t } = useTranslation();
  const { isDarkMode, toggleDarkMode } = useDarkMode();
  const { isAutoSyncEnabled } = useVSCodeAutoSync();
  const queryClient = useQueryClient();
  const [activeApp, setActiveApp] = useState<AppType>("claude");
  const [isAddModalOpen, setIsAddModalOpen] = useState(false);
  const [editingProviderId, setEditingProviderId] = useState<string | null>(
    null
  );
  const [notification, setNotification] = useState<{
    message: string;
    type: "success" | "error";
  } | null>(null);
  const [isNotificationVisible, setIsNotificationVisible] = useState(false);
  const [confirmDialog, setConfirmDialog] = useState<{
    isOpen: boolean;
    title: string;
    message: string;
    onConfirm: () => void;
  } | null>(null);
  const [isSettingsOpen, setIsSettingsOpen] = useState(false);
  const timeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Query hooks
  const { data: providersData, isLoading, error } = useProvidersQuery(activeApp);
  const addProviderMutation = useAddProviderMutation(activeApp);
  const updateProviderMutation = useUpdateProviderMutation(activeApp);
  const vscodeSyncMutation = useVSCodeSyncMutation(activeApp);

  const providers: Record<string, Provider> = providersData?.providers || Object.create(null);
  const currentProviderId = (providersData?.currentProviderId as string) || "";

  // 设置通知的辅助函数
  const showNotification = (
    message: string,
    type: "success" | "error",
    duration = 3000
  ) => {
    // 清除之前的定时器
    if (timeoutRef.current) {
      clearTimeout(timeoutRef.current);
    }

    // 立即显示通知
    setNotification({ message, type });
    setIsNotificationVisible(true);

    // 设置淡出定时器
    timeoutRef.current = setTimeout(() => {
      setIsNotificationVisible(false);
      // 等待淡出动画完成后清除通知
      setTimeout(() => {
        setNotification(null);
        timeoutRef.current = null;
      }, 300); // 与CSS动画时间匹配
    }, duration);
  };

  // 清理定时器
  useEffect(() => {
    return () => {
      if (timeoutRef.current) {
        clearTimeout(timeoutRef.current);
      }
    };
  }, []);

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

  
  const handleAddProvider = (provider: Omit<Provider, "id">) => {
    addProviderMutation.mutate(provider, {
      onSuccess: () => {
        setIsAddModalOpen(false);
        showNotification(t("notifications.providerAdded"), "success", 2000);
      },
      onError: (error) => {
        console.error(t("console.addProviderFailed"), error);
        const errorMessage = extractErrorMessage(error);
        const message = errorMessage
          ? t("notifications.addFailed", { error: errorMessage })
          : t("notifications.addFailedGeneric");
        showNotification(message, "error", errorMessage ? 6000 : 3000);
      }
    });
  };

  const handleEditProvider = (provider: Provider) => {
    updateProviderMutation.mutate(provider, {
      onSuccess: () => {
        setEditingProviderId(null);
        showNotification(t("notifications.providerSaved"), "success", 2000);
      },
      onError: (error) => {
        console.error(t("console.updateProviderFailed"), error);
        setEditingProviderId(null);
        const errorMessage = extractErrorMessage(error);
        const message = errorMessage
          ? t("notifications.saveFailed", { error: errorMessage })
          : t("notifications.saveFailedGeneric");
        showNotification(message, "error", errorMessage ? 6000 : 3000);
      }
    });
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
            <button
              onClick={toggleDarkMode}
              className={buttonStyles.icon}
              title={
                isDarkMode
                  ? t("header.toggleLightMode")
                  : t("header.toggleDarkMode")
              }
            >
              {isDarkMode ? <Sun size={18} /> : <Moon size={18} />}
            </button>
            <div className="flex items-center gap-2">
              <button
                onClick={() => setIsSettingsOpen(true)}
                className={buttonStyles.icon}
                title={t("common.settings")}
              >
                <Settings size={18} />
              </button>
              <UpdateBadge onClick={() => setIsSettingsOpen(true)} />
            </div>
          </div>

          <div className="flex items-center gap-4">
            <AppSwitcher activeApp={activeApp} onSwitch={setActiveApp} />

            <button
              onClick={() => setIsAddModalOpen(true)}
              className={`inline-flex items-center gap-2 ${buttonStyles.primary}`}
            >
              <Plus size={16} />
              {t("header.addProvider")}
            </button>
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

            {notification && (
              <div
                className={`fixed top-20 left-1/2 transform -translate-x-1/2 z-50 px-4 py-3 rounded-lg shadow-lg transition-all duration-300 ${
                  notification.type === "error"
                    ? "bg-red-500 text-white"
                    : "bg-green-500 text-white"
                } ${isNotificationVisible ? "opacity-100 translate-y-0" : "opacity-0 -translate-y-2"}`}
              >
                {notification.message}
              </div>
            )}

            {!isLoading && !error && (
              <ProviderList
                providers={providers}
                currentProviderId={currentProviderId}
                onEdit={setEditingProviderId}
                appType={activeApp}
                onNotify={showNotification}
              />
            )}
          </div>
        </div>
      </main>

      {isAddModalOpen && (
        <AddProviderModal
          appType={activeApp}
          onAdd={handleAddProvider}
          onClose={() => setIsAddModalOpen(false)}
        />
      )}

      {editingProviderId && providers[editingProviderId] && (
        <EditProviderModal
          appType={activeApp}
          provider={providers[editingProviderId]}
          onSave={handleEditProvider}
          onClose={() => setEditingProviderId(null)}
        />
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

      {isSettingsOpen && (
        <SettingsModal onClose={() => setIsSettingsOpen(false)} />
      )}
    </div>
  );
}

export default App;
