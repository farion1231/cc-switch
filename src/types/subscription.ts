export type CredentialStatus =
  | "valid"
  | "expired"
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

export interface RateLimitResetCredit {
  grantedAt: string | null;
  expiresAt: string | null;
  title: string | null;
  description: string | null;
}

export interface RateLimitResetCredits {
  /** 权威的可用数量；明细接口可能只返回部分卡片。 */
  availableCount: number;
  /** null 表示明细查询失败，空数组表示查询成功但没有可展示的可用卡。 */
  credits: RateLimitResetCredit[] | null;
}

export interface CodexMembership {
  /** 当前 ChatGPT 订阅周期结束时间；不是 OAuth/JWT token 的过期时间。 */
  activeUntil: string;
  /** true 表示届时续费，false 表示届时到期，null 表示数据源未提供。 */
  willRenew: boolean | null;
}

export interface SubscriptionQuota {
  tool: string;
  credentialStatus: CredentialStatus;
  credentialMessage: string | null;
  success: boolean;
  tiers: QuotaTier[];
  extraUsage: ExtraUsage | null;
  /** Codex / ChatGPT 套餐类型。 */
  planType?: string | null;
  /** Codex / ChatGPT 当前订阅周期；私有订阅元数据不可用时省略。 */
  membership?: CodexMembership | null;
  /** Codex 限速重置卡；旧响应与其他工具可省略。 */
  rateLimitResetCredits?: RateLimitResetCredits | null;
  error: string | null;
  queriedAt: number | null;
}
