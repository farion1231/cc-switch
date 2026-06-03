import { invoke } from "@tauri-apps/api/core";
import type {
  TokenBudget,
  CreateTokenBudgetInput,
  UpdateTokenBudgetInput,
  BudgetStatus,
} from "@/types/budget";

export const budgetApi = {
  list: async (): Promise<TokenBudget[]> => {
    return invoke("list_token_budgets");
  },

  create: async (input: CreateTokenBudgetInput): Promise<TokenBudget> => {
    return invoke("create_token_budget", { input });
  },

  update: async (
    id: string,
    patch: UpdateTokenBudgetInput,
  ): Promise<TokenBudget> => {
    return invoke("update_token_budget", { id, patch });
  },

  delete: async (id: string): Promise<void> => {
    return invoke("delete_token_budget", { id });
  },

  getStatus: async (id: string): Promise<BudgetStatus | null> => {
    return invoke("get_token_budget_status", { id });
  },

  getAllStatuses: async (): Promise<BudgetStatus[]> => {
    return invoke("get_all_token_budget_statuses");
  },
};
