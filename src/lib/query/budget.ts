import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { budgetApi } from "@/lib/api/budget";
import type {
  CreateTokenBudgetInput,
  UpdateTokenBudgetInput,
} from "@/types/budget";

// ── Query Key 工厂 ──────────────────────────────────────────────

export const budgetKeys = {
  all: ["budget"] as const,
  list: () => [...budgetKeys.all, "list"] as const,
  statuses: () => [...budgetKeys.all, "statuses"] as const,
  status: (id: string) => [...budgetKeys.all, "status", id] as const,
};

// ── Queries ─────────────────────────────────────────────────────

/** 获取所有预算列表 */
export function useBudgets() {
  return useQuery({
    queryKey: budgetKeys.list(),
    queryFn: budgetApi.list,
  });
}

/** 获取所有预算状态（Dashboard 首屏一次拉取） */
export function useBudgetStatuses() {
  return useQuery({
    queryKey: budgetKeys.statuses(),
    queryFn: budgetApi.getAllStatuses,
    refetchInterval: 30000,
  });
}

/** 获取单个预算状态 */
export function useBudgetStatus(id: string) {
  return useQuery({
    queryKey: budgetKeys.status(id),
    queryFn: () => budgetApi.getStatus(id),
    enabled: !!id,
  });
}

// ── Mutations ───────────────────────────────────────────────────

/** 创建预算 */
export function useCreateBudget() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: CreateTokenBudgetInput) => budgetApi.create(input),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: budgetKeys.all });
    },
  });
}

/** 更新预算 */
export function useUpdateBudget() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (args: { id: string; patch: UpdateTokenBudgetInput }) =>
      budgetApi.update(args.id, args.patch),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: budgetKeys.all });
    },
  });
}

/** 删除预算 */
export function useDeleteBudget() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => budgetApi.delete(id),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: budgetKeys.all });
    },
  });
}
