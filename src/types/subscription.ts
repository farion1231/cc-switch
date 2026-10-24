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

// ===== codex-resets.com 社区重置预测 =====

/** 重置公告 / 观测的来源（"x_post" 推文 或 "observed" 社区观测） */
export interface CodexResetSource {
  kind: string;
  url: string | null;
}

/** 已执行的一次重置（最近一次） */
export interface CodexResetEntry {
  id: string;
  /** "regular" | "banked" */
  resetType: string;
  /** 公告或首次观测到的时间（ISO 8601） */
  announcedAt: string;
  text: string | null;
  source: CodexResetSource;
}

/** 已公告、还没等到执行证据的一次重置；scheduledFor 过期不代表已执行 */
export interface CodexScheduledReset {
  id: string;
  /** "regular" | "banked" */
  resetType: string;
  announcedAt: string;
  scheduledFor: string | null;
  text: string | null;
  source: CodexResetSource;
}

/** 进行中的重置观测（AI 分类的预测，非 OpenAI 官方承诺） */
export interface CodexResetWatch {
  /** "elevated" | "strong" */
  level: string;
  resetChancePercent: number | null;
  forecastWindow: string;
  observedAt: string;
  expiresAt: string;
  text: string | null;
  source: CodexResetSource;
}

export interface CodexResetsStats {
  total: number;
  lastResetAt: string | null;
  daysSinceLast: number | null;
  avgIntervalDays: number | null;
}

export interface CodexResetsStatus {
  latestReset: CodexResetEntry | null;
  scheduledReset: CodexScheduledReset | null;
  activeWatch: CodexResetWatch | null;
  stats: CodexResetsStats;
  /** 本地抓取时间（毫秒时间戳） */
  fetchedAt: number;
}
