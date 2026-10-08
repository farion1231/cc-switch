import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { mcpApi } from "@/lib/api/mcp";
import type { McpServer, McpServersMap } from "@/types";
import type { AppId } from "@/lib/api/types";
import { runSequentialBulkAction } from "@/lib/utils/sequentialBulkAction";

/**
 * 查询所有 MCP 服务器（统一管理）
 */
export function useAllMcpServers() {
  return useQuery({
    queryKey: ["mcp", "all"],
    queryFn: () => mcpApi.getAllServers(),
  });
}

/**
 * 添加或更新 MCP 服务器
 */
/** 编辑页保存用的 mutation key：页面据此在保存进行中锁住导航 */
export const MCP_UPSERT_MUTATION_KEY = ["mcp", "upsert"] as const;

export function useUpsertMcpServer() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationKey: MCP_UPSERT_MUTATION_KEY,
    mutationFn: (server: McpServer) => mcpApi.upsertUnifiedServer(server),
    // The database is updated before live configs are synchronized, so an
    // error can still leave a persisted change that the list must reflect.
    onSettled: () =>
      queryClient.invalidateQueries({ queryKey: ["mcp", "all"] }),
  });
}

/** Toggle multiple MCP servers serially to avoid lost whole-file writes. */
export function useBulkToggleMcpApp() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({
      serverIds,
      app,
      enabled,
    }: {
      serverIds: string[];
      app: AppId;
      enabled: boolean;
    }) =>
      runSequentialBulkAction(serverIds, (serverId) =>
        mcpApi.toggleApp(serverId, app, enabled),
      ),
    onSettled: () =>
      queryClient.invalidateQueries({ queryKey: ["mcp", "all"] }),
  });
}

/**
 * 切换 MCP 服务器在特定应用的启用状态
 */
export function useToggleMcpApp() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({
      serverId,
      app,
      enabled,
    }: {
      serverId: string;
      app: AppId;
      enabled: boolean;
    }) => mcpApi.toggleApp(serverId, app, enabled),
    // 乐观更新：点了格子立刻翻过来，不等写完再刷新；失败时回滚
    onMutate: async ({ serverId, app, enabled }) => {
      await queryClient.cancelQueries({ queryKey: ["mcp", "all"] });
      const previous = queryClient.getQueryData<McpServersMap>(["mcp", "all"]);
      const server = previous?.[serverId];
      if (previous && server) {
        queryClient.setQueryData<McpServersMap>(["mcp", "all"], {
          ...previous,
          [serverId]: { ...server, apps: { ...server.apps, [app]: enabled } },
        });
      }
      return { previous };
    },
    onError: (_error, _vars, context) => {
      if (context?.previous) {
        queryClient.setQueryData(["mcp", "all"], context.previous);
      }
    },
    // The backend may update the database before a live-config write fails.
    // Always refresh so the UI reflects the persisted state after an error.
    onSettled: () =>
      queryClient.invalidateQueries({ queryKey: ["mcp", "all"] }),
  });
}

/**
 * 删除 MCP 服务器
 */
export function useDeleteMcpServer() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => mcpApi.deleteUnifiedServer(id),
    // Deletion reaches the database before live-config cleanup, so refresh
    // after both success and failure to avoid operating on a removed entry.
    onSettled: () =>
      queryClient.invalidateQueries({ queryKey: ["mcp", "all"] }),
  });
}

/**
 * 按这里的开关把 MCP 重新写进各应用的配置（不传 apps＝全部受管应用），逐应用返回结果
 */
export function useResyncMcpToApps() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (apps?: AppId[]) => mcpApi.resyncToApps(apps),
    onSettled: () =>
      queryClient.invalidateQueries({ queryKey: ["mcp", "all"] }),
  });
}

/**
 * 从所有应用导入 MCP 服务器
 */
export function useImportMcpFromApps() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: () => mcpApi.importFromApps(),
    // 后端是 best-effort 导入：部分应用失败会返回错误，但其余应用的
    // 服务器已经入库，失败时也要刷新列表。
    onSettled: () =>
      queryClient.invalidateQueries({ queryKey: ["mcp", "all"] }),
  });
}
