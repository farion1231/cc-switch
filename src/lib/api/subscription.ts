import { invoke } from "@tauri-apps/api/core";
import type { SubscriptionQuota } from "@/types/subscription";

export interface VolcengineAkSkStatus {
  configured: boolean;
  kind: "referenced" | "default" | "legacy" | "migrated" | "none";
  accountLabel?: string;
  akMasked?: string;
  skMasked?: string;
}

export const subscriptionApi = {
  getQuota: (tool: string): Promise<SubscriptionQuota> =>
    invoke("get_subscription_quota", { tool }),
  getCodexOauthQuota: (accountId: string | null): Promise<SubscriptionQuota> =>
    invoke("get_codex_oauth_quota", { accountId }),
  getXaiOauthQuota: (accountId: string | null): Promise<SubscriptionQuota> =>
    invoke("get_xai_oauth_quota", { accountId }),
  getCodingPlanQuota: (
    baseUrl: string,
    apiKey: string,
    // 火山方舟用账号 AK/SK 签名查询用量；其他供应商不传。
    accessKeyId?: string,
    secretAccessKey?: string,
    // 智谱团队版（zhipu_team）靠显式标识路由（base_url 与个人版相同无法区分）。
    codingPlanProvider?: string,
    teamOrganizationId?: string,
    teamProjectId?: string,
    // 火山账号池：条目引用的账号 id + 条目名（惰性迁移入池时作默认名称）。
    akskAccountId?: string,
    providerName?: string,
  ): Promise<SubscriptionQuota> =>
    invoke("get_coding_plan_quota", {
      baseUrl,
      apiKey,
      accessKeyId,
      secretAccessKey,
      codingPlanProvider,
      teamOrganizationId,
      teamProjectId,
      akskAccountId,
      providerName,
    }),
  // ── 火山账号池 ──
  listVolcengineAccounts: (): Promise<{ id: string; label: string }[]> =>
    invoke("list_volcengine_accounts"),
  saveVolcengineAccount: (input: {
    id?: string;
    label?: string;
    accessKeyId: string;
    secretAccessKey: string;
  }): Promise<{ id: string; label: string }> =>
    invoke("save_volcengine_account", { request: input }),
  deleteVolcengineAccount: (id: string): Promise<void> =>
    invoke("delete_volcengine_account", { id }),
  renameVolcengineAccount: (id: string, label: string): Promise<void> =>
    invoke("rename_volcengine_account", { id, label }),
  getVolcengineAkSkStatus: (
    app: string,
    providerId: string,
  ): Promise<VolcengineAkSkStatus> =>
    invoke("get_volcengine_aksk_status", { app, providerId }),
  getBalance: (
    baseUrl: string,
    apiKey: string,
  ): Promise<import("@/types").UsageResult> =>
    invoke("get_balance", { baseUrl, apiKey }),
};
