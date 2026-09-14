import { invoke } from "@tauri-apps/api/core";
import type {
  CodexQuotaActivationPolicy,
  SubscriptionQuota,
} from "@/types/subscription";

export const subscriptionApi = {
  getQuota: (tool: string): Promise<SubscriptionQuota> =>
    invoke("get_subscription_quota", { tool }),
  getCodexOauthQuota: (accountId: string | null): Promise<SubscriptionQuota> =>
    invoke("get_codex_oauth_quota", { accountId }),
  getCodexQuotaActivationPolicy: (
    providerId: string,
  ): Promise<CodexQuotaActivationPolicy | null> =>
    invoke("get_codex_quota_activation_policy", { providerId }),
  setCodexQuotaActivationPolicy: (
    providerId: string,
    enabled: boolean,
  ): Promise<CodexQuotaActivationPolicy> =>
    invoke("set_codex_quota_activation_policy", { providerId, enabled }),
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
  ): Promise<SubscriptionQuota> =>
    invoke("get_coding_plan_quota", {
      baseUrl,
      apiKey,
      accessKeyId,
      secretAccessKey,
      codingPlanProvider,
      teamOrganizationId,
      teamProjectId,
    }),
  getBalance: (
    baseUrl: string,
    apiKey: string,
  ): Promise<import("@/types").UsageResult> =>
    invoke("get_balance", { baseUrl, apiKey }),
};
