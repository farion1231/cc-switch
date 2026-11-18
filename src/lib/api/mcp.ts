import { Channel, invoke } from "@tauri-apps/api/core";
import type { McpServer, McpServersMap } from "@/types";
import type { AppId } from "./types";

export interface McpConnectivityResult {
  serverId: string;
  /** `authRequired`: the server answered 401 and wants the client's own sign-in. */
  status: "connected" | "authRequired" | "failed";
}

export const mcpApi = {
  async startConnectivityChecks(
    serverIds: string[],
    onResult: (result: McpConnectivityResult) => void,
  ): Promise<string> {
    const channel = new Channel<McpConnectivityResult>();
    channel.onmessage = onResult;
    return await invoke("start_mcp_connectivity_checks", {
      serverIds,
      onResult: channel,
    });
  },

  async cancelConnectivityChecks(batchId: string): Promise<void> {
    await invoke("cancel_mcp_connectivity_checks", { batchId });
  },

  // ========================================================================
  // v3.7.0 新增：统一 MCP 管理 API
  // ========================================================================

  /**
   * 获取所有 MCP 服务器（统一结构）
   */
  async getAllServers(): Promise<McpServersMap> {
    return await invoke("get_mcp_servers");
  },

  /**
   * 添加或更新 MCP 服务器（统一结构）
   */
  async upsertUnifiedServer(server: McpServer): Promise<void> {
    return await invoke("upsert_mcp_server", { server });
  },

  /**
   * 删除 MCP 服务器
   */
  async deleteUnifiedServer(id: string): Promise<boolean> {
    return await invoke("delete_mcp_server", { id });
  },

  /**
   * 切换 MCP 服务器在指定应用的启用状态
   */
  async toggleApp(
    serverId: string,
    app: AppId,
    enabled: boolean,
  ): Promise<void> {
    return await invoke("toggle_mcp_app", { serverId, app, enabled });
  },

  /**
   * 从所有应用导入 MCP 服务器
   */
  async importFromApps(): Promise<number> {
    return await invoke("import_mcp_from_apps");
  },

  /**
   * 按这里的开关把 MCP 重新写进各应用的配置，逐应用返回结果。
   * 不传 apps 时同步全部受管应用；单个应用失败不影响其余应用。
   */
  async resyncToApps(apps?: AppId[]): Promise<McpAppSyncOutcome[]> {
    return await invoke("resync_mcp_to_apps", { apps: apps ?? null });
  },
};

/** 「重新同步到各应用」里单个应用的结果 */
export interface McpAppSyncOutcome {
  app: AppId;
  ok: boolean;
  error?: string;
}
