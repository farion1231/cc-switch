import { invoke } from "@tauri-apps/api/core";
import type {
  RouteSwitchResult,
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
  PublicRouteStatus,
  NamedTunnel,
  CursorByokStatus,
  CursorByokBackup,
  CursorRestoreByokResult,
  CursorWriteByokResult,
  PublicRouteConnectivityResult,
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
  async setProxyRoute(
    appType: string,
    providerId: string,
  ): Promise<RouteSwitchResult> {
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

  // 关掉看不到进程时出的「Codex 可能还在用旧的」提示：后端记下现在的模型目录和登录，同一份
  // 不再提示
  async acknowledgeCodexStaleClients(): Promise<void> {
    return invoke("acknowledge_codex_stale_clients");
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

  // 获取客户端地址（写给客户端的代理主机，空表示按监听地址推）
  async getProxyClientHost(): Promise<string> {
    return invoke("get_proxy_client_host");
  },

  // 保存客户端地址，并按新地址重写接上路由的客户端
  async setProxyClientHost(host: string): Promise<void> {
    return invoke("set_proxy_client_host", { host });
  },

  // 获取指定应用的代理配置
  async getProxyConfigForApp(appType: string): Promise<AppProxyConfig> {
    return invoke("get_proxy_config_for_app", { appType });
  },

  // 更新指定应用的代理配置
  async updateProxyConfigForApp(config: AppProxyConfig): Promise<void> {
    return invoke("update_proxy_config_for_app", { config });
  },

  // ========== 公网路由 API ==========

  // 查询公网路由状态
  async getPublicRouteStatus(): Promise<PublicRouteStatus> {
    return invoke("get_public_route_status");
  },

  // 启用公网路由（启动 cloudflared 隧道）
  async enablePublicRoute(): Promise<PublicRouteStatus> {
    return invoke("enable_public_route");
  },

  // 禁用公网路由（停止隧道）
  async disablePublicRoute(): Promise<PublicRouteStatus> {
    return invoke("disable_public_route");
  },

  // 重新生成公网路由隧道鉴权密钥（ccsk-*）；Cursor 的 key 无法自动写入，需用户重新粘贴
  async regeneratePublicRouteApiKey(): Promise<PublicRouteStatus> {
    return invoke("regenerate_public_route_api_key");
  },

  // 保存公网路由隧道配置（模式 + 命名隧道参数）；开启中会立即重建隧道
  async setPublicRouteTunnelConfig(config: {
    mode: string;
    namedTunnel?: string | null;
    namedHostname?: string | null;
  }): Promise<PublicRouteStatus> {
    return invoke("set_public_route_tunnel_config", config);
  },

  // 列出 Cloudflare 账户下已有的命名隧道（需已 cloudflared login）
  async listNamedTunnels(): Promise<NamedTunnel[]> {
    return invoke("list_named_tunnels");
  },

  // ========== Cursor 接入引导 API ==========

  // Cursor 是否正在运行：运行中会把设置回写覆盖，写 BYOK 前必须先完全退出
  async cursorIsRunning(): Promise<boolean> {
    return invoke("cursor_is_running");
  },

  // 读 Cursor 本地 BYOK 现状（Base URL / 是否用自定义 Key / 可用模型数）。Cursor 运行中读到的是
  // 过期快照，判断「是否已写好」要在它退出后再读
  async cursorByokStatus(): Promise<CursorByokStatus> {
    return invoke("cursor_byok_status");
  },

  // 写 Cursor BYOK：Base URL + 可用模型列表。密钥是加密存储的，读不出也写不进（需手工粘）
  async cursorWriteByok(params: {
    baseUrl: string;
    modelIds: string[];
  }): Promise<CursorWriteByokResult> {
    return invoke("cursor_write_byok", params);
  },

  // 最新的 BYOK 回滚点（没有则 null）。退出接管后据此决定是否提示「一键还原」
  async cursorByokBackup(): Promise<CursorByokBackup | null> {
    return invoke("cursor_byok_backup");
  },

  // 还原 BYOK 到最新回滚点（写前的原始行）。只提示不代劳，由用户在退出接管后显式点击
  async cursorRestoreByok(): Promise<CursorRestoreByokResult> {
    return invoke("cursor_restore_byok");
  },

  // 分层测公网路由连通性：local 打本机代理，public 打隧道公网地址，用来定位坏在哪一层
  async testPublicRouteConnectivity(
    scope: "local" | "public",
  ): Promise<PublicRouteConnectivityResult> {
    return invoke("test_public_route_connectivity", { scope });
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
