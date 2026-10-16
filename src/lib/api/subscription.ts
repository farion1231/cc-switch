import { invoke } from "@tauri-apps/api/core";
import type { SubscriptionQuota } from "@/types/subscription";

export interface CodexQuotaActivationState {
  accountId: string;
  enabled: boolean;
  effectiveIntervalMinutes?: number | null;
  enabledAtMs?: number | null;
  lastPolledAtMs?: number | null;
  lastPollStatus?: string | null;
  lastPollReason?: string | null;
  lastAttemptAtMs?: number | null;
  lastAttemptStatus?: string | null;
  lastReason?: string | null;
  stateError?: string | null;
  authorizationRevision: number;
  state: { kind: string; target?: string; eventKey?: string };
  updatedAt: number;
}

export const subscriptionApi = {
  getQuota: (tool: string): Promise<SubscriptionQuota> =>
    invoke("get_subscription_quota", { tool }),
  getCodexOauthQuota: (accountId: string | null): Promise<SubscriptionQuota> =>
    invoke("get_codex_oauth_quota", { accountId }),
  getCodexQuotaActivationState: (
    accountId: string,
  ): Promise<CodexQuotaActivationState | null> =>
    invoke("get_codex_quota_activation_state", { accountId }),
  listCodexQuotaActivationStates: (): Promise<CodexQuotaActivationState[]> =>
    invoke("list_codex_quota_activation_states"),
  setCodexQuotaActivationEnabled: (
    accountId: string,
    enabled: boolean,
    effectiveIntervalMinutes?: number,
  ): Promise<CodexQuotaActivationState> =>
    invoke("set_codex_quota_activation_enabled", {
      accountId,
      enabled,
      effectiveIntervalMinutes,
    }),
  pollCodexOauthQuotaForActivation: (
    accountId: string,
  ): Promise<{
    accountId: string;
    polled: boolean;
    attempted: boolean;
    target?: string | null;
    status: string;
    reason?: string | null;
    quota?: SubscriptionQuota | null;
  }> =>
    invoke("poll_codex_oauth_quota_for_activation", {
      accountId,
    }),
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
