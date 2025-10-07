import { invoke } from "@tauri-apps/api/core";
import { listen, UnlistenFn } from "@tauri-apps/api/event";
import { Provider, Settings, CustomEndpoint } from "../types";

// 应用类型
export type AppType = "claude" | "codex";

// 定义配置状态类型
interface ConfigStatus {
  exists: boolean;
  path: string;
  error?: string;
}

// 定义导入结果类型
interface ImportResult {
  success: boolean;
  message?: string;
}

export interface EndpointLatencyResult {
  url: string;
  latency: number | null;
  status?: number;
  error?: string;
}

// Tauri API 封装，提供统一的全局 API 接口
export const tauriAPI = {
  // 获取所有供应商
  getProviders: async (app?: AppType): Promise<Record<string, Provider>> => {
    try {
      return await invoke("get_providers", { app_type: app, app });
    } catch (error) {
      console.error("获取供应商列表失败:", error);
      return {};
    }
  },

  // 获取当前供应商ID
  getCurrentProvider: async (app?: AppType): Promise<string> => {
    try {
      return await invoke("get_current_provider", { app_type: app, app });
    } catch (error) {
      console.error("获取当前供应商失败:", error);
      return "";
    }
  },

  // 添加供应商
  addProvider: async (provider: Provider, app?: AppType): Promise<boolean> => {
    try {
      return await invoke("add_provider", { provider, app_type: app, app });
    } catch (error) {
      console.error("添加供应商失败:", error);
      throw error;
    }
  },

  // 更新供应商
  updateProvider: async (
    provider: Provider,
    app?: AppType,
  ): Promise<boolean> => {
    try {
      return await invoke("update_provider", { provider, app_type: app, app });
    } catch (error) {
      console.error("更新供应商失败:", error);
      throw error;
    }
  },

  // 删除供应商
  deleteProvider: async (id: string, app?: AppType): Promise<boolean> => {
    try {
      return await invoke("delete_provider", { id, app_type: app, app });
    } catch (error) {
      console.error("删除供应商失败:", error);
      throw error;
    }
  },

  // 切换供应商
  switchProvider: async (
    providerId: string,
    app?: AppType,
  ): Promise<boolean> => {
    try {
      return await invoke("switch_provider", {
        id: providerId,
        app_type: app,
        app,
      });
    } catch (error) {
      console.error("切换供应商失败:", error);
      return false;
    }
  },

  // 导入当前配置为默认供应商
  importCurrentConfigAsDefault: async (
    app?: AppType,
  ): Promise<ImportResult> => {
    try {
      const success = await invoke<boolean>("import_default_config", {
        app_type: app,
        app,
      });
      return {
        success,
        message: success ? "成功导入默认配置" : "导入失败",
      };
    } catch (error) {
      console.error("导入默认配置失败:", error);
      return {
        success: false,
        message: String(error),
      };
    }
  },

  // 获取 Claude Code 配置文件路径
  getClaudeCodeConfigPath: async (): Promise<string> => {
    try {
      return await invoke("get_claude_code_config_path");
    } catch (error) {
      console.error("获取配置路径失败:", error);
      return "";
    }
  },

  // 获取当前生效的配置目录
  getConfigDir: async (app?: AppType): Promise<string> => {
    try {
      return await invoke("get_config_dir", { app_type: app, app });
    } catch (error) {
      console.error("获取配置目录失败:", error);
      return "";
    }
  },

  // 打开配置目录（按应用类型）
  openConfigFolder: async (app?: AppType): Promise<void> => {
    try {
      await invoke("open_config_folder", { app_type: app, app });
    } catch (error) {
      console.error("打开配置目录失败:", error);
    }
  },

  // 选择配置目录（可选默认路径）
  selectConfigDirectory: async (defaultPath?: string): Promise<string | null> => {
    try {
      return await invoke("pick_directory", { defaultPath });
    } catch (error) {
      console.error("选择配置目录失败:", error);
      return null;
    }
  },

  // 打开外部链接
  openExternal: async (url: string): Promise<void> => {
    try {
      await invoke("open_external", { url });
    } catch (error) {
      console.error("打开外部链接失败:", error);
    }
  },

  // 更新托盘菜单
  updateTrayMenu: async (): Promise<boolean> => {
    try {
      return await invoke<boolean>("update_tray_menu");
    } catch (error) {
      console.error("更新托盘菜单失败:", error);
      return false;
    }
  },

  // 获取应用设置
  getSettings: async (): Promise<Settings> => {
    try {
      return await invoke("get_settings");
    } catch (error) {
      console.error("获取设置失败:", error);
      throw error;
    }
  },

  // 保存设置
  saveSettings: async (settings: Settings): Promise<boolean> => {
    try {
      return await invoke("save_settings", { settings });
    } catch (error) {
      console.error("保存设置失败:", error);
      return false;
    }
  },

  // 检查更新
  checkForUpdates: async (): Promise<void> => {
    try {
      await invoke("check_for_updates");
    } catch (error) {
      console.error("检查更新失败:", error);
    }
  },

  // 判断是否为便携模式
  isPortable: async (): Promise<boolean> => {
    try {
      return await invoke<boolean>("is_portable_mode");
    } catch (error) {
      console.error("检测便携模式失败:", error);
      return false;
    }
  },

  // 获取应用配置文件路径
  getAppConfigPath: async (): Promise<string> => {
    try {
      return await invoke("get_app_config_path");
    } catch (error) {
      console.error("获取应用配置路径失败:", error);
      return "";
    }
  },

  // 打开应用配置文件夹
  openAppConfigFolder: async (): Promise<void> => {
    try {
      await invoke("open_app_config_folder");
    } catch (error) {
      console.error("打开应用配置文件夹失败:", error);
    }
  },

  // Claude 插件：获取 ~/.claude/config.json 状态
  getClaudePluginStatus: async (): Promise<ConfigStatus> => {
    try {
      return await invoke<ConfigStatus>("get_claude_plugin_status");
    } catch (error) {
      console.error("获取 Claude 插件状态失败:", error);
      return { exists: false, path: "", error: String(error) };
    }
  },

  // Claude 插件：读取配置内容
  readClaudePluginConfig: async (): Promise<string | null> => {
    try {
      return await invoke<string | null>("read_claude_plugin_config");
    } catch (error) {
      throw new Error(`读取 Claude 插件配置失败: ${String(error)}`);
    }
  },

  // Claude 插件：应用或移除固定配置
  applyClaudePluginConfig: async (options: {
    official: boolean;
  }): Promise<boolean> => {
    const { official } = options;
    try {
      return await invoke<boolean>("apply_claude_plugin_config", { official });
    } catch (error) {
      throw new Error(`写入 Claude 插件配置失败: ${String(error)}`);
    }
  },

  // Claude 插件：检测是否已应用目标配置
  isClaudePluginApplied: async (): Promise<boolean> => {
    try {
      return await invoke<boolean>("is_claude_plugin_applied");
    } catch (error) {
      throw new Error(`检测 Claude 插件配置失败: ${String(error)}`);
    }
  },

  // ours: 第三方/自定义供应商——测速与端点管理
  // 第三方/自定义供应商：批量测试端点延迟
  testApiEndpoints: async (
    urls: string[],
    options?: { timeoutSecs?: number },
  ): Promise<EndpointLatencyResult[]> => {
    try {
      return await invoke<EndpointLatencyResult[]>("test_api_endpoints", {
        urls,
        timeout_secs: options?.timeoutSecs,
      });
    } catch (error) {
      console.error("测速调用失败:", error);
      throw error;
    }
  },

  // 获取自定义端点列表
  getCustomEndpoints: async (
    appType: AppType,
    providerId: string,
  ): Promise<CustomEndpoint[]> => {
    try {
      return await invoke<CustomEndpoint[]>("get_custom_endpoints", {
        // 兼容不同后端参数命名
        app_type: appType,
        app: appType,
        appType: appType,
        provider_id: providerId,
        providerId: providerId,
      });
    } catch (error) {
      console.error("获取自定义端点列表失败:", error);
      return [];
    }
  },

  // 添加自定义端点
  addCustomEndpoint: async (
    appType: AppType,
    providerId: string,
    url: string,
  ): Promise<void> => {
    try {
      await invoke("add_custom_endpoint", {
        app_type: appType,
        app: appType,
        appType: appType,
        provider_id: providerId,
        providerId: providerId,
        url,
      });
    } catch (error) {
      console.error("添加自定义端点失败:", error);
      // 尽量抛出可读信息
      if (error instanceof Error) {
        throw error;
      } else {
        throw new Error(String(error));
      }
    }
  },

  // 删除自定义端点
  removeCustomEndpoint: async (
    appType: AppType,
    providerId: string,
    url: string,
  ): Promise<void> => {
    try {
      await invoke("remove_custom_endpoint", {
        app_type: appType,
        app: appType,
        appType: appType,
        provider_id: providerId,
        providerId: providerId,
        url,
      });
    } catch (error) {
      console.error("删除自定义端点失败:", error);
      throw error;
    }
  },

  // 更新端点最后使用时间
  updateEndpointLastUsed: async (
    appType: AppType,
    providerId: string,
    url: string,
  ): Promise<void> => {
    try {
      await invoke("update_endpoint_last_used", {
        app_type: appType,
        app: appType,
        appType: appType,
        provider_id: providerId,
        providerId: providerId,
        url,
      });
    } catch (error) {
      console.error("更新端点最后使用时间失败:", error);
      // 不抛出错误，因为这不是关键操作
    }
  },

  // theirs: 导入导出与文件对话框
  // 导出配置到文件
  exportConfigToFile: async (filePath: string): Promise<{
    success: boolean;
    message: string;
    filePath: string;
  }> => {
    try {
      return await invoke("export_config_to_file", { filePath });
    } catch (error) {
      throw new Error(`导出配置失败: ${String(error)}`);
    }
  },

  // 从文件导入配置
  importConfigFromFile: async (filePath: string): Promise<{
    success: boolean;
    message: string;
    backupId?: string;
  }> => {
    try {
      return await invoke("import_config_from_file", { filePath });
    } catch (error) {
      throw new Error(`导入配置失败: ${String(error)}`);
    }
  },

  // 保存文件对话框
  saveFileDialog: async (defaultName: string): Promise<string | null> => {
    try {
      const result = await invoke<string | null>("save_file_dialog", { defaultName });
      return result;
    } catch (error) {
      console.error("打开保存对话框失败:", error);
      return null;
    }
  },

  // 打开文件对话框
  openFileDialog: async (): Promise<string | null> => {
    try {
      const result = await invoke<string | null>("open_file_dialog");
      return result;
    } catch (error) {
      console.error("打开文件对话框失败:", error);
      return null;
    }
  },

  // 监听供应商切换事件
  onProviderSwitched: async (
    callback: (data: { appType: string; providerId: string }) => void,
  ): Promise<UnlistenFn> => {
    const unlisten = await listen("provider-switched", (event) => {
      try {
        // 事件 payload 形如 { appType: string, providerId: string }
        callback(event.payload as any);
      } catch (e) {
        console.error("处理 provider-switched 事件失败: ", e);
      }
    });
    return unlisten;
  },
};

// 创建全局 API 对象，兼容现有代码
if (typeof window !== "undefined") {
  // 绑定到 window.api，避免 Electron 命名造成误解
  // API 内部已做 try/catch，非 Tauri 环境下也会安全返回默认值
  (window as any).api = tauriAPI;
}

export default tauriAPI;
