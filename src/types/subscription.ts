export type CredentialStatus =
  | "valid"
  | "expired"
  // 访问令牌过期、刷新令牌还在：客户端下次运行时自己会换新的
  | "refresh_pending"
  | "not_found"
  | "parse_error";

export interface QuotaTier {
  name: string;
  utilization: number; // 0-100
  resetsAt: string | null;
  usedValueUsd?: number | null;
  maxValueUsd?: number | null;
  planLabel?: string | null;
}

export interface ExtraUsage {
  isEnabled: boolean;
  monthlyLimit: number | null;
  usedCredits: number | null;
  utilization: number | null;
  currency: string | null;
}

/** ChatGPT 订阅存下的限额重置：每一次的到期时间，先到期的在前，null 表示不过期 */
export interface ResetCredits {
  expiresAt: (string | null)[];
}

export interface SubscriptionQuota {
  tool: string;
  credentialStatus: CredentialStatus;
  credentialMessage: string | null;
  success: boolean;
  tiers: QuotaTier[];
  extraUsage: ExtraUsage | null;
  /** 只有 ChatGPT 订阅有；没查到时缺省 */
  resetCredits?: ResetCredits | null;
  /** ChatGPT 订阅买的 Codex Credits 余额（额度用完后才扣）；没有时缺省 */
  creditsBalance?: number | null;
  error: string | null;
  queriedAt: number | null;
}
