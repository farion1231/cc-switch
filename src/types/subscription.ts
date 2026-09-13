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

export interface CodexQuotaActivationAttempt {
  status: string;
  windowType: string;
  model: string | null;
  reasoningEffort: string | null;
  startedAt: number;
  endedAt: number | null;
  exitCode: number | null;
  error: string | null;
}

export interface CodexQuotaActivationPolicy {
  accountId: string;
  ownerProviderId: string;
  ownerProviderName: string | null;
  enabled: boolean;
  canEdit: boolean;
  updatedAt: number;
  lastStatus: string | null;
  lastError: string | null;
  lastWindowType: string | null;
  lastModel: string | null;
  lastAttemptAt: number | null;
  latestAttempt: CodexQuotaActivationAttempt | null;
}
