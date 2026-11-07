import { useCallback } from "react";
import {
  keepPreviousData,
  useMutation,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query";
import {
  subagentsApi,
  type SubagentDriftStrategy,
  type SubagentInput,
} from "@/lib/api/subagents";

export const subagentKeys = {
  all: ["subagents"] as const,
  list: ["subagents", "list"] as const,
  dir: ["subagents", "dir"] as const,
  unmanaged: ["subagents", "unmanaged"] as const,
  plugins: ["subagents", "plugins"] as const,
  disk: (id: string) => ["subagents", "disk", id] as const,
  /**
   * 模型选项跟着 Claude Code 当前的供应商和模式走，所以挂在 ["providers", "claude"] 下面：
   * 切换供应商、进出路由 / 聚合、编辑供应商、托盘切换后，各处都会按前缀失效
   * ["providers", app]（见 App.tsx 的 onSwitched、useModeActions、mutations.ts），
   * 这里不用再一处处补失效。
   */
  modelOptions: ["providers", "claude", "subagent-model-options"] as const,
};

export function useSubagentsQuery() {
  return useQuery({
    queryKey: subagentKeys.list,
    queryFn: () => subagentsApi.list(),
    placeholderData: keepPreviousData,
  });
}

export function useSubagentsDirQuery() {
  return useQuery({
    queryKey: subagentKeys.dir,
    queryFn: () => subagentsApi.getDir(),
    staleTime: 30_000,
  });
}

export function useUnmanagedSubagentsQuery() {
  return useQuery({
    queryKey: subagentKeys.unmanaged,
    queryFn: () => subagentsApi.scanUnmanaged(),
    placeholderData: keepPreviousData,
  });
}

export function usePluginSubagentsQuery() {
  return useQuery({
    queryKey: subagentKeys.plugins,
    queryFn: () => subagentsApi.listPlugin(),
    placeholderData: keepPreviousData,
  });
}

/** 偏离对话框打开时读磁盘上的那份；每次打开都重读，不用旧缓存 */
export function useSubagentDiskContentQuery(id: string | null) {
  return useQuery({
    queryKey: subagentKeys.disk(id ?? ""),
    queryFn: () => subagentsApi.getDiskContent(id as string),
    enabled: id !== null,
    staleTime: 0,
    gcTime: 0,
  });
}

/** 编辑器挂载时总是重新拉一次（环境变量、供应商可能在别处改过） */
export function useSubagentModelOptionsQuery() {
  return useQuery({
    queryKey: subagentKeys.modelOptions,
    queryFn: () => subagentsApi.getModelOptions(),
    refetchOnMount: "always",
  });
}

/** 写完之后库、未管理列表、磁盘内容都可能变了：整组失效 */
export function useInvalidateSubagents() {
  const queryClient = useQueryClient();
  return useCallback(
    () => queryClient.invalidateQueries({ queryKey: subagentKeys.all }),
    [queryClient],
  );
}

export function useUpsertSubagent() {
  const invalidate = useInvalidateSubagents();
  return useMutation({
    mutationFn: ({ input, force }: { input: SubagentInput; force?: boolean }) =>
      subagentsApi.upsert(input, force),
    onSettled: invalidate,
  });
}

export function useDeleteSubagent() {
  const invalidate = useInvalidateSubagents();
  return useMutation({
    mutationFn: ({ id, force }: { id: string; force?: boolean }) =>
      subagentsApi.delete(id, force),
    onSettled: invalidate,
  });
}

export function useToggleSubagent() {
  const invalidate = useInvalidateSubagents();
  return useMutation({
    mutationFn: ({
      id,
      enabled,
      force,
    }: {
      id: string;
      enabled: boolean;
      force?: boolean;
    }) => subagentsApi.toggle(id, enabled, force),
    onSettled: invalidate,
  });
}

export function useImportSubagents() {
  const invalidate = useInvalidateSubagents();
  return useMutation({
    mutationFn: (fileNames: string[]) => subagentsApi.import(fileNames),
    onSettled: invalidate,
  });
}

export function useResolveSubagentDrift() {
  const invalidate = useInvalidateSubagents();
  return useMutation({
    mutationFn: ({
      id,
      strategy,
    }: {
      id: string;
      strategy: SubagentDriftStrategy;
    }) => subagentsApi.resolveDrift(id, strategy),
    onSettled: invalidate,
  });
}
