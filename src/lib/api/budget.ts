import { invoke } from "@tauri-apps/api/core";
import type {
  TokenBudget,
  CreateTokenBudgetInput,
  UpdateTokenBudgetInput,
  BudgetStatus,
} from "@/types/budget";

/** Strip undefined & NaN — JSON.stringify silently drops undefined but turns NaN into null,
 *  which serde can't deserialize into i32/Option<i64>. */
function cleanInput<T extends Record<string, unknown>>(obj: T): T {
  return Object.fromEntries(
    Object.entries(obj).filter(
      ([_, v]) =>
        v !== undefined && (typeof v !== "number" || !Number.isNaN(v)),
    ),
  ) as T;
}

export const budgetApi = {
  list: async (): Promise<TokenBudget[]> => {
    return invoke("list_token_budgets");
  },

  create: async (input: CreateTokenBudgetInput): Promise<TokenBudget> => {
    console.log("[budgetApi.create] input:", JSON.stringify(input, null, 2));
    if (typeof invoke !== "function") {
      throw new Error(
        "Tauri invoke 不可用 — 请确认在 Tauri 应用内运行，而非浏览器",
      );
    }
    const clean = cleanInput(input as unknown as Record<string, unknown>);
    console.log(
      "[budgetApi.create] clean input:",
      JSON.stringify(clean, null, 2),
    );
    try {
      const result = await invoke("create_token_budget", { input: clean });
      console.log("[budgetApi.create] result:", result);
      return result as TokenBudget;
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : String(e);
      console.error("[budgetApi.create] invoke failed:", msg);
      throw new Error(`创建预算失败: ${msg}`);
    }
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
