import {
  useMutation,
  useQuery,
  useQueryClient,
  keepPreviousData,
} from "@tanstack/react-query";
import {
  agentsApi,
  type AgentBackupEntry,
  type DiscoverableAgent,
  type ImportAgentSelection,
  type InstalledAgent,
} from "@/lib/api/agents";
import type { AppId } from "@/lib/api/types";

function upsertInstalledAgentsById(
  oldData: InstalledAgent[] | undefined,
  incomingAgents: InstalledAgent | InstalledAgent[],
) {
  const agents = Array.isArray(incomingAgents)
    ? incomingAgents
    : [incomingAgents];
  const nextAgents = new Map((oldData ?? []).map((agent) => [agent.id, agent]));
  agents.forEach((agent) => {
    nextAgents.set(agent.id, agent);
  });

  return Array.from(nextAgents.values());
}

/**
 * 查询所有已安装的 Agents
 */
export function useInstalledAgents() {
  return useQuery({
    queryKey: ["agents", "installed"],
    queryFn: () => agentsApi.getInstalled(),
    staleTime: Infinity,
    placeholderData: keepPreviousData,
  });
}

export function useAgentBackups() {
  return useQuery({
    queryKey: ["agents", "backups"],
    queryFn: () => agentsApi.getBackups(),
    enabled: false,
  });
}

export function useDeleteAgentBackup() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (backupId: string) => agentsApi.deleteBackup(backupId),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["agents", "backups"] });
    },
  });
}

/**
 * 发现可安装的 Agents（从仓库获取）
 */
export function useDiscoverableAgents() {
  return useQuery({
    queryKey: ["agents", "discoverable"],
    queryFn: () => agentsApi.discoverAvailable(),
    staleTime: Infinity,
    placeholderData: keepPreviousData,
  });
}

/**
 * 安装 Agent
 * 成功后直接更新缓存，不触发重新加载/刷新
 */
export function useInstallAgent() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({
      agent,
      currentApp,
    }: {
      agent: DiscoverableAgent;
      currentApp: AppId;
    }) => agentsApi.installUnified(agent, currentApp),
    onSuccess: (installedAgent, _vars, _ctx) => {
      const { agent } = _vars;
      queryClient.setQueryData<InstalledAgent[]>(
        ["agents", "installed"],
        (oldData) => upsertInstalledAgentsById(oldData, installedAgent),
      );

      queryClient.setQueryData<DiscoverableAgent[]>(
        ["agents", "discoverable"],
        (oldData) => {
          if (!oldData) return oldData;
          return oldData.map((a) => {
            if (a.key === agent.key) {
              return { ...a, installed: true };
            }
            return a;
          });
        },
      );
    },
  });
}

/**
 * 卸载 Agent
 * 成功后直接更新缓存，不触发重新加载/刷新
 */
export function useUninstallAgent() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ id, agentKey }: { id: string; agentKey: string }) =>
      agentsApi
        .uninstallUnified(id)
        .then((result) => ({ ...result, agentKey })),
    onSuccess: ({ agentKey }, _vars) => {
      queryClient.setQueryData<InstalledAgent[]>(
        ["agents", "installed"],
        (oldData) => {
          if (!oldData) return oldData;
          return oldData.filter((a) => a.id !== _vars.id);
        },
      );

      queryClient.setQueryData<DiscoverableAgent[]>(
        ["agents", "discoverable"],
        (oldData) => {
          if (!oldData) return oldData;
          return oldData.map((a) => {
            if (a.key === agentKey) {
              return { ...a, installed: false };
            }
            return a;
          });
        },
      );
    },
  });
}

export function useRestoreAgentBackup() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({
      backupId,
      currentApp,
    }: {
      backupId: string;
      currentApp: AppId;
    }) => agentsApi.restoreBackup(backupId, currentApp),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["agents", "installed"] });
      queryClient.invalidateQueries({ queryKey: ["agents", "backups"] });
    },
  });
}

/**
 * 切换 Agent 在特定应用的启用状态
 */
export function useToggleAgentApp() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({
      id,
      app,
      enabled,
    }: {
      id: string;
      app: AppId;
      enabled: boolean;
    }) => agentsApi.toggleApp(id, app, enabled),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["agents", "installed"] });
    },
  });
}

/**
 * 扫描未管理的 Agents
 */
export function useScanUnmanagedAgents() {
  return useQuery({
    queryKey: ["agents", "unmanaged"],
    queryFn: () => agentsApi.scanUnmanaged(),
    enabled: false,
  });
}

/**
 * 从应用目录导入 Agents
 * 成功后直接更新缓存，不触发重新加载/刷新
 */
export function useImportAgentsFromApps() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (imports: ImportAgentSelection[]) =>
      agentsApi.importFromApps(imports),
    onSuccess: (importedAgents) => {
      queryClient.setQueryData<InstalledAgent[]>(
        ["agents", "installed"],
        (oldData) => upsertInstalledAgentsById(oldData, importedAgents),
      );
      queryClient.invalidateQueries({ queryKey: ["agents", "unmanaged"] });
    },
  });
}

/**
 * 获取仓库列表
 */
export function useAgentRepos() {
  return useQuery({
    queryKey: ["agents", "repos"],
    queryFn: () => agentsApi.getRepos(),
  });
}

/**
 * 添加仓库
 */
export function useAddAgentRepo() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: agentsApi.addRepo,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["agents", "repos"] });
      queryClient.invalidateQueries({ queryKey: ["agents", "discoverable"] });
    },
  });
}

/**
 * 删除仓库
 */
export function useRemoveAgentRepo() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ owner, name }: { owner: string; name: string }) =>
      agentsApi.removeRepo(owner, name),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["agents", "repos"] });
      queryClient.invalidateQueries({ queryKey: ["agents", "discoverable"] });
    },
  });
}

/**
 * 从 ZIP 文件安装 Agents
 * 成功后直接更新缓存，不触发重新加载/刷新
 */
export function useInstallAgentsFromZip() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({
      filePath,
      currentApp,
    }: {
      filePath: string;
      currentApp: AppId;
    }) => agentsApi.installFromZip(filePath, currentApp),
    onSuccess: (installedAgents) => {
      queryClient.setQueryData<InstalledAgent[]>(
        ["agents", "installed"],
        (oldData) => upsertInstalledAgentsById(oldData, installedAgents),
      );
    },
  });
}

// ========== 辅助类型 ==========

export type {
  InstalledAgent,
  DiscoverableAgent,
  ImportAgentSelection,
  AgentBackupEntry,
  AppId,
};
