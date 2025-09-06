import { useState, useEffect, useRef } from "react";
import { Provider } from "./types";
import { AppType } from "./lib/tauri-api";
import ProviderList from "./components/ProviderList";
import AddProviderModal from "./components/AddProviderModal";
import EditProviderModal from "./components/EditProviderModal";
import { ConfirmDialog } from "./components/ConfirmDialog";
import { AppSwitcher } from "./components/AppSwitcher";
import { Plus } from "lucide-react";

function App() {
  const [activeApp, setActiveApp] = useState<AppType>("claude");
  const [providers, setProviders] = useState<Record<string, Provider>>({});
  const [currentProviderId, setCurrentProviderId] = useState<string>("");
  const [isAddModalOpen, setIsAddModalOpen] = useState(false);
  const [configStatus, setConfigStatus] = useState<{
    exists: boolean;
    path: string;
  } | null>(null);
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
  const timeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);

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

  // 加载供应商列表
  useEffect(() => {
    loadProviders();
    loadConfigStatus();
  }, [activeApp]); // 当切换应用时重新加载

  // 清理定时器
  useEffect(() => {
    return () => {
      if (timeoutRef.current) {
        clearTimeout(timeoutRef.current);
      }
    };
  }, []);

  // 监听托盘切换事件
  useEffect(() => {
    let unlisten: (() => void) | null = null;

    const setupListener = async () => {
      try {
        unlisten = await window.api.onProviderSwitched(async (data) => {
          console.log("收到供应商切换事件:", data);

          // 如果当前应用类型匹配，则重新加载数据
          if (data.appType === activeApp) {
            await loadProviders();
          }
        });
      } catch (error) {
        console.error("设置供应商切换监听器失败:", error);
      }
    };

    setupListener();

    // 清理监听器
    return () => {
      if (unlisten) {
        unlisten();
      }
    };
  }, [activeApp]); // 依赖activeApp，切换应用时重新设置监听器

  const loadProviders = async () => {
    const loadedProviders = await window.api.getProviders(activeApp);
    const currentId = await window.api.getCurrentProvider(activeApp);
    setProviders(loadedProviders);
    setCurrentProviderId(currentId);

    // 如果供应商列表为空，尝试自动从 live 导入一条默认供应商
    if (Object.keys(loadedProviders).length === 0) {
      await handleAutoImportDefault();
    }
  };

  const loadConfigStatus = async () => {
    const status = await window.api.getConfigStatus(activeApp);
    setConfigStatus({
      exists: Boolean(status?.exists),
      path: String(status?.path || ""),
    });
  };

  // 生成唯一ID
  const generateId = () => {
    return crypto.randomUUID();
  };

  const handleAddProvider = async (provider: Omit<Provider, "id">) => {
    const newProvider: Provider = {
      ...provider,
      id: generateId(),
    };
    await window.api.addProvider(newProvider, activeApp);
    await loadProviders();
    setIsAddModalOpen(false);
    // 更新托盘菜单
    await window.api.updateTrayMenu();
  };

  const handleEditProvider = async (provider: Provider) => {
    try {
      await window.api.updateProvider(provider, activeApp);
      await loadProviders();
      setEditingProviderId(null);
      // 显示编辑成功提示
      showNotification("供应商配置已保存", "success", 2000);
      // 更新托盘菜单
      await window.api.updateTrayMenu();
    } catch (error) {
      console.error("更新供应商失败:", error);
      setEditingProviderId(null);
      showNotification("保存失败，请重试", "error");
    }
  };

  const handleDeleteProvider = async (id: string) => {
    const provider = providers[id];
    setConfirmDialog({
      isOpen: true,
      title: "删除供应商",
      message: `确定要删除供应商 "${provider?.name}" 吗？此操作无法撤销。`,
      onConfirm: async () => {
        await window.api.deleteProvider(id, activeApp);
        await loadProviders();
        setConfirmDialog(null);
        showNotification("供应商删除成功", "success");
        // 更新托盘菜单
        await window.api.updateTrayMenu();
      },
    });
  };

  const handleSwitchProvider = async (id: string) => {
    const success = await window.api.switchProvider(id, activeApp);
    if (success) {
      setCurrentProviderId(id);
      // 显示重启提示
      const appName = activeApp === "claude" ? "Claude Code" : "Codex";
      showNotification(
        `切换成功！请重启 ${appName} 终端以生效`,
        "success",
        2000
      );
      // 更新托盘菜单
      await window.api.updateTrayMenu();
    } else {
      showNotification("切换失败，请检查配置", "error");
    }
  };

  // 自动从 live 导入一条默认供应商（仅首次初始化时）
  const handleAutoImportDefault = async () => {
    try {
      const result = await window.api.importCurrentConfigAsDefault(activeApp);

      if (result.success) {
        await loadProviders();
        showNotification("已从现有配置创建默认供应商", "success", 3000);
        // 更新托盘菜单
        await window.api.updateTrayMenu();
      }
      // 如果导入失败（比如没有现有配置），静默处理，不显示错误
    } catch (error) {
      console.error("自动导入默认配置失败:", error);
      // 静默处理，不影响用户体验
    }
  };

  const handleOpenConfigFolder = async () => {
    await window.api.openConfigFolder(activeApp);
  };

  return (
    <div className="min-h-screen flex flex-col bg-[var(--color-bg-primary)]">
      {/* Linear 风格的顶部导航 */}
      <header className="bg-white border-b border-[var(--color-border)] px-6 py-4">
        <div className="flex items-center justify-between">
          <h1 className="text-xl font-semibold text-[var(--color-text-primary)]">
            CC Switch
          </h1>

          <div className="flex items-center gap-4">
            <AppSwitcher activeApp={activeApp} onSwitch={setActiveApp} />

            <button
              onClick={() => setIsAddModalOpen(true)}
              className="inline-flex items-center gap-2 px-4 py-2 bg-[var(--color-primary)] text-white rounded-lg hover:bg-[var(--color-primary-hover)] transition-colors text-sm font-medium"
            >
              <Plus size={16} />
              添加供应商
            </button>
          </div>
        </div>
      </header>

      {/* 主内容区域 */}
      <main className="flex-1 p-6">
        <div className="max-w-4xl mx-auto">
          {/* 通知组件 */}
          {notification && (
            <div
              className={`fixed top-6 left-1/2 transform -translate-x-1/2 z-50 px-4 py-3 rounded-lg shadow-lg transition-all duration-300 ${
                notification.type === "error"
                  ? "bg-[var(--color-error)] text-white"
                  : "bg-[var(--color-success)] text-white"
              } ${isNotificationVisible ? "opacity-100 translate-y-0" : "opacity-0 -translate-y-2"}`}
            >
              {notification.message}
            </div>
          )}

          <ProviderList
            providers={providers}
            currentProviderId={currentProviderId}
            onSwitch={handleSwitchProvider}
            onDelete={handleDeleteProvider}
            onEdit={setEditingProviderId}
          />

          {/* 配置文件路径信息 */}
          {configStatus && (
            <div className="mt-8 p-4 bg-white rounded-lg border border-[var(--color-border)]">
              <div className="flex items-center justify-between">
                <div className="text-sm text-[var(--color-text-secondary)]">
                  <span className="font-medium">
                    {activeApp === "claude" ? "Claude Code" : "Codex"}{" "}
                    配置文件位置:
                  </span>
                  <span className="ml-2 font-mono text-xs">
                    {configStatus.path}
                  </span>
                  {!configStatus.exists && (
                    <span className="ml-2 text-[var(--color-warning)]">
                      （未创建，切换或保存时会自动创建）
                    </span>
                  )}
                </div>
                <button
                  onClick={handleOpenConfigFolder}
                  className="px-3 py-1.5 text-sm font-medium text-[var(--color-primary)] hover:bg-[var(--color-bg-tertiary)] rounded-md transition-colors"
                  title="打开配置文件夹"
                >
                  打开文件夹
                </button>
              </div>
            </div>
          )}
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
    </div>
  );
}

export default App;
