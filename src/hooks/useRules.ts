import {
  useMutation,
  useQuery,
  useQueryClient,
  keepPreviousData,
} from "@tanstack/react-query";
import {
  rulesApi,
  type RuleBackupEntry,
  type DiscoverableRule,
  type ImportRuleSelection,
  type InstalledRule,
} from "@/lib/api/rules";
import type { AppId } from "@/lib/api/types";

function upsertInstalledRulesById(
  oldData: InstalledRule[] | undefined,
  incomingRules: InstalledRule | InstalledRule[],
) {
  const rules = Array.isArray(incomingRules) ? incomingRules : [incomingRules];
  const nextRules = new Map((oldData ?? []).map((rule) => [rule.id, rule]));
  rules.forEach((rule) => {
    nextRules.set(rule.id, rule);
  });

  return Array.from(nextRules.values());
}

/**
 * 查询所有已安装的 Rules
 */
export function useInstalledRules() {
  return useQuery({
    queryKey: ["rules", "installed"],
    queryFn: () => rulesApi.getInstalled(),
    staleTime: Infinity,
    placeholderData: keepPreviousData,
  });
}

export function useRuleBackups() {
  return useQuery({
    queryKey: ["rules", "backups"],
    queryFn: () => rulesApi.getBackups(),
    enabled: false,
  });
}

export function useDeleteRuleBackup() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (backupId: string) => rulesApi.deleteBackup(backupId),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["rules", "backups"] });
    },
  });
}

/**
 * 发现可安装的 Rules（从仓库获取）
 */
export function useDiscoverableRules() {
  return useQuery({
    queryKey: ["rules", "discoverable"],
    queryFn: () => rulesApi.discoverAvailable(),
    staleTime: Infinity,
    placeholderData: keepPreviousData,
  });
}

/**
 * 安装 Rule
 * 成功后直接更新缓存，不触发重新加载/刷新
 */
export function useInstallRule() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({
      rule,
      currentApp,
    }: {
      rule: DiscoverableRule;
      currentApp: AppId;
    }) => rulesApi.installUnified(rule, currentApp),
    onSuccess: (installedRule, _vars, _ctx) => {
      const { rule } = _vars;
      queryClient.setQueryData<InstalledRule[]>(
        ["rules", "installed"],
        (oldData) => upsertInstalledRulesById(oldData, installedRule),
      );

      queryClient.setQueryData<DiscoverableRule[]>(
        ["rules", "discoverable"],
        (oldData) => {
          if (!oldData) return oldData;
          return oldData.map((r) => {
            if (r.key === rule.key) {
              return { ...r, installed: true };
            }
            return r;
          });
        },
      );
    },
  });
}

/**
 * 卸载 Rule
 * 成功后直接更新缓存，不触发重新加载/刷新
 */
export function useUninstallRule() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ id, ruleKey }: { id: string; ruleKey: string }) =>
      rulesApi.uninstallUnified(id).then((result) => ({ ...result, ruleKey })),
    onSuccess: ({ ruleKey }, _vars) => {
      queryClient.setQueryData<InstalledRule[]>(
        ["rules", "installed"],
        (oldData) => {
          if (!oldData) return oldData;
          return oldData.filter((r) => r.id !== _vars.id);
        },
      );

      queryClient.setQueryData<DiscoverableRule[]>(
        ["rules", "discoverable"],
        (oldData) => {
          if (!oldData) return oldData;
          return oldData.map((r) => {
            if (r.key === ruleKey) {
              return { ...r, installed: false };
            }
            return r;
          });
        },
      );
    },
  });
}

export function useRestoreRuleBackup() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({
      backupId,
      currentApp,
    }: {
      backupId: string;
      currentApp: AppId;
    }) => rulesApi.restoreBackup(backupId, currentApp),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["rules", "installed"] });
      queryClient.invalidateQueries({ queryKey: ["rules", "backups"] });
    },
  });
}

/**
 * 切换 Rule 在特定应用的启用状态
 */
export function useToggleRuleApp() {
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
    }) => rulesApi.toggleApp(id, app, enabled),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["rules", "installed"] });
    },
  });
}

/**
 * 扫描未管理的 Rules
 */
export function useScanUnmanagedRules() {
  return useQuery({
    queryKey: ["rules", "unmanaged"],
    queryFn: () => rulesApi.scanUnmanaged(),
    enabled: false,
  });
}

/**
 * 从应用目录导入 Rules
 * 成功后直接更新缓存，不触发重新加载/刷新
 */
export function useImportRulesFromApps() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (imports: ImportRuleSelection[]) =>
      rulesApi.importFromApps(imports),
    onSuccess: (importedRules) => {
      queryClient.setQueryData<InstalledRule[]>(
        ["rules", "installed"],
        (oldData) => upsertInstalledRulesById(oldData, importedRules),
      );
      queryClient.invalidateQueries({ queryKey: ["rules", "unmanaged"] });
    },
  });
}

/**
 * 获取仓库列表
 */
export function useRuleRepos() {
  return useQuery({
    queryKey: ["rules", "repos"],
    queryFn: () => rulesApi.getRepos(),
  });
}

/**
 * 添加仓库
 */
export function useAddRuleRepo() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: rulesApi.addRepo,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["rules", "repos"] });
      queryClient.invalidateQueries({ queryKey: ["rules", "discoverable"] });
    },
  });
}

/**
 * 删除仓库
 */
export function useRemoveRuleRepo() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ owner, name }: { owner: string; name: string }) =>
      rulesApi.removeRepo(owner, name),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["rules", "repos"] });
      queryClient.invalidateQueries({ queryKey: ["rules", "discoverable"] });
    },
  });
}

/**
 * 从 ZIP 文件安装 Rules
 * 成功后直接更新缓存，不触发重新加载/刷新
 */
export function useInstallRulesFromZip() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({
      filePath,
      currentApp,
    }: {
      filePath: string;
      currentApp: AppId;
    }) => rulesApi.installFromZip(filePath, currentApp),
    onSuccess: (installedRules) => {
      queryClient.setQueryData<InstalledRule[]>(
        ["rules", "installed"],
        (oldData) => upsertInstalledRulesById(oldData, installedRules),
      );
    },
  });
}

// ========== 辅助类型 ==========

export type {
  InstalledRule,
  DiscoverableRule,
  ImportRuleSelection,
  RuleBackupEntry,
  AppId,
};
