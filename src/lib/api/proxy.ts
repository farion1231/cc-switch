import { invoke } from "@tauri-apps/api/core";
import type {
  ProxyStatus,
  ProxyServerInfo,
  ProxyTakeoverStatus,
  GlobalProxyConfig,
  AppProxyConfig,
  ProxyStack,
  ProxyStackNotice,
  CodexDaemonRestartOutcome,
  AppModeView,
  StartupAttachFailure,
} from "@/types/proxy";

export const proxyApi = {
  // ========== 代理服务器控制 API ==========

  // 启动代理服务器
  async startProxyServer(): Promise<ProxyServerInfo> {
    return invoke("start_proxy_server");
  },

  // 停止代理服务器（不恢复已接管配置）
  async stopProxyServer(): Promise<void> {
    return invoke("stop_proxy_server");
  },

  // 停止代理服务器并恢复配置
  async stopProxyWithRestore(): Promise<void> {
    return invoke("stop_proxy_with_restore");
  },

  // 获取代理服务器状态
  async getProxyStatus(): Promise<ProxyStatus> {
    return invoke("get_proxy_status");
  },

  // ========== 接管状态 API ==========

  // 获取各应用接管状态
  async getProxyTakeoverStatus(): Promise<ProxyTakeoverStatus> {
    return invoke("get_proxy_takeover_status");
  },

  // 为指定应用进入/退出代理模式。stack 为真时进入的是 Stack 模式（和路由模式二选一）；
  // route 是确认框里选的路由目标（Stack 模式下是默认那家），不传沿用上次的路由
  async setProxyTakeoverForApp(
    appType: string,
    enabled: boolean,
    stack = false,
    route?: string | null,
  ): Promise<void> {
    return invoke("set_proxy_takeover_for_app", {
      appType,
      enabled,
      stack,
      route: route ?? null,
    });
  },

  // 指定路由目标（聚合模式下是默认那家）：直连时只记下来、下次进入路由 / 聚合模式时用它，
  // 已经在路由 / 聚合模式时当场生效
  async setProxyRoute(appType: string, providerId: string): Promise<void> {
    return invoke("set_proxy_route", { appType, providerId });
  },

  // 应用页模式行：生效的模式、路由目标（直连时是上次路由的那家）、直连那家
  async getAppMode(appType: string): Promise<AppModeView> {
    return invoke("get_app_mode", { appType });
  },

  // 启动时没能接上代理、已退回直连的应用（取一次就清空）
  async takeStartupAttachFailures(): Promise<StartupAttachFailure[]> {
    return invoke("take_startup_attach_failures");
  },

  // 设置里在路由和 Stack 之间换时：处于另一种模式（stack 为真是 Stack 模式）的 Claude Code、
  // Codex 先退回直连。返回退回直连的应用
  async exitProxyAppsInMode(stack: boolean): Promise<string[]> {
    return invoke("exit_proxy_apps_in_mode", { stack });
  },

  // 直连供应商：路由模式下退出路由时写回的那家（和路由到的那家互相独立）
  async getDirectProvider(appType: string): Promise<string | null> {
    return invoke("get_direct_provider", { appType });
  },

  // ========== Stack 模型 API ==========

  // Stack 模型名单：每一家和它发布的模型 id，以及提示
  async getProxyStack(appType: string): Promise<ProxyStack> {
    return invoke("get_proxy_stack", { appType });
  },

  // 把一家加入或移出 Stack 模型（enabled 是目标值）。成功时返回客户端看不到或看不全 Stack 模型
  // 的提示；失败时抛出 ProxyStackWriteError
  async setProxyStackMember(
    appType: string,
    providerId: string,
    enabled: boolean,
  ): Promise<ProxyStackNotice | null> {
    return invoke("set_proxy_stack_member", { appType, providerId, enabled });
  },

  // Codex 聚合的模型被别的模型目录挡住时，改用 CC Switch 生成的目录（去掉指向别的文件的
  // model_catalog_json）。返回之后还剩的提示
  async adoptCodexStackCatalog(): Promise<ProxyStackNotice | null> {
    return invoke("adopt_codex_stack_catalog");
  },

  // 重启 Codex 的托管守护进程（codex 命令行连的那个），让它重读模型目录。会中断正在运行的
  // 任务，只在用户确认之后调
  async restartCodexAppServerDaemon(): Promise<CodexDaemonRestartOutcome> {
    return invoke("restart_codex_app_server_daemon");
  },

  // ========== v3+ 全局/应用级配置 API ==========

  // 获取全局代理配置
  async getGlobalProxyConfig(): Promise<GlobalProxyConfig> {
    return invoke("get_global_proxy_config");
  },

  // 更新全局代理配置
  async updateGlobalProxyConfig(config: GlobalProxyConfig): Promise<void> {
    return invoke("update_global_proxy_config", { config });
  },

  // 获取指定应用的代理配置
  async getProxyConfigForApp(appType: string): Promise<AppProxyConfig> {
    return invoke("get_proxy_config_for_app", { appType });
  },

  // 更新指定应用的代理配置
  async updateProxyConfigForApp(config: AppProxyConfig): Promise<void> {
    return invoke("update_proxy_config_for_app", { config });
  },

  // ========== 计费默认配置 API ==========

  // 获取计费模式来源
  async getPricingModelSource(appType: string): Promise<string> {
    return invoke("get_pricing_model_source", { appType });
  },

  // 设置计费模式来源
  async setPricingModelSource(appType: string, value: string): Promise<void> {
    return invoke("set_pricing_model_source", { appType, value });
  },
};
