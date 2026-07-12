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
  // MiniMax 等 provider 直接返回绝对次数时填充；与 utilization 同时存在，
  // 渲染层应优先用绝对值（"7 / 100"）展示。仅在 provider 给出绝对值时存在，
  // 其余 provider（ZenMux USD / Zhipu percentage）保持 undefined。
  usedCount?: number | null;
  totalCount?: number | null;
  countUnit?: string | null;
  // 服务端精确剩余毫秒数（来自 MiniMax *_remains_time 等字段）。
  // 优先于前端 end_time - Date.now() 计算（避免本地时钟漂移 + 拿到毫秒精度）。
  remainsTimeMs?: number | null;
}

export interface ExtraUsage {
  isEnabled: boolean;
  monthlyLimit: number | null;
  usedCredits: number | null;
  utilization: number | null;
  currency: string | null;
}

export interface SubscriptionQuota {
  tool: string;
  credentialStatus: CredentialStatus;
  credentialMessage: string | null;
  success: boolean;
  tiers: QuotaTier[];
  extraUsage: ExtraUsage | null;
  error: string | null;
  queriedAt: number | null;
}
