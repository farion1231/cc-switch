import { get, post, del } from "../web-client";
import type { McpServer, McpServersMap } from "@/types";
import type { AppId } from "./types";

// Web (HTTP) implementation of the unified MCP API.
// Must expose the same surface consumed by the shared hooks (src/hooks/useMcp.ts)
// so that `mcpApi = isTauri() ? tauriMcpApi : webMcpApi` stays interchangeable.
export const mcpApi = {
  /**
   * 获取所有 MCP 服务器（统一结构，id -> McpServer）
   */
  async getAllServers(): Promise<McpServersMap> {
    return get("/mcp");
  },

  /**
   * 添加或更新 MCP 服务器（统一结构）
   */
  async upsertUnifiedServer(server: McpServer): Promise<void> {
    await post("/mcp", server);
  },

  /**
   * 删除 MCP 服务器
   */
  async deleteUnifiedServer(id: string): Promise<boolean> {
    return del(`/mcp/${encodeURIComponent(id)}`);
  },

  /**
   * 切换 MCP 服务器在指定应用的启用状态
   */
  async toggleApp(
    serverId: string,
    app: AppId,
    enabled: boolean,
  ): Promise<void> {
    await post(`/mcp/${encodeURIComponent(serverId)}/toggle`, { app, enabled });
  },

  /**
   * 从所有应用导入 MCP 服务器（服务端读取本地各应用配置）
   */
  async importFromApps(): Promise<number> {
    // Send an empty object so the server's JSON body extractor succeeds and
    // falls through to the "import from local app configs" path.
    return post("/mcp/import", {});
  },
};
