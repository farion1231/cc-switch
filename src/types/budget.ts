// Token Budget 类型定义
// 映射 src-tauri/src/token_budget.rs + services/token_budget.rs
// Rust 端 #[serde(rename_all = "camelCase")] → TS camelCase
// 时间单位：秒（与 proxy_request_logs.created_at 一致）

/** 预算作用域 */
export type BudgetScope = "global" | "app" | "provider" | "model";

/** 预算周期 */
export type BudgetPeriod = "daily" | "weekly" | "monthly";

/** 预算定义（L0） */
export interface TokenBudget {
  id: string;
  name: string;
  scope: BudgetScope;
  scopeValue?: string; // scope=global 时不存在; provider 时为 "app_type:provider_id"
  period: BudgetPeriod;
  periodStartDay: number;
  limitTokens?: number; // i64
  limitUsd?: string; // Decimal 字符串 "10.500000"
  enabled: boolean;
  createdAt?: number; // unix sec
  updatedAt?: number;
}

/** 创建预算输入 */
export interface CreateTokenBudgetInput {
  name: string;
  scope: BudgetScope;
  scopeValue?: string;
  period: BudgetPeriod;
  periodStartDay?: number; // 默认 1
  limitTokens?: number;
  limitUsd?: string;
  enabled?: boolean; // 默认 true
}

/** 更新预算输入（所有字段可选） */
export interface UpdateTokenBudgetInput {
  name?: string;
  scope?: BudgetScope;
  scopeValue?: string | null; // null = 清除
  period?: BudgetPeriod;
  periodStartDay?: number;
  limitTokens?: number | null; // null = 清除
  limitUsd?: string | null; // null = 清除
  enabled?: boolean;
}

/** 周期窗口（半开区间 [startSec, endSec)），单位：秒 */
export interface BudgetWindow {
  startSec: number;
  endSec: number;
}

/** 预算实时状态（L1） */
export interface BudgetStatus {
  budget: TokenBudget;
  window: BudgetWindow;
  /** 当前窗口内已消费的 real_total_tokens（cache 归一化） */
  consumedTokens: number;
  /** 当前窗口内已消费的 USD，6 位小数字符串 */
  consumedUsd: string;
  /** tokens 维度进度 0.0~；>1.0 表示超额；undefined=未设 tokens 上限 */
  pctTokens?: number;
  /** usd 维度进度；undefined=未设 usd 上限 */
  pctUsd?: number;
  /** 剩余 tokens（可为负） */
  remainingTokens?: number;
  /** 剩余 USD（可为负），6 位小数字符串 */
  remainingUsd?: string;
}

/** scope 对应的显示标签 key */
export const SCOPE_LABEL_KEYS: Record<BudgetScope, string> = {
  global: "budget.scopeGlobal",
  app: "budget.scopeApp",
  provider: "budget.scopeProvider",
  model: "budget.scopeModel",
};

/** period 对应的显示标签 key */
export const PERIOD_LABEL_KEYS: Record<BudgetPeriod, string> = {
  daily: "budget.periodDaily",
  weekly: "budget.periodWeekly",
  monthly: "budget.periodMonthly",
};
