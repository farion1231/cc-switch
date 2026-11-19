import { invoke } from "@tauri-apps/api/core";
import type { SubscriptionQuota } from "@/types/subscription";

export type McodeRegion = "cn" | "en";

export interface McodeOAuthAccount {
  id: string;
  name: string;
  email: string | null;
  avatarUrl: string | null;
  region: McodeRegion;
  authenticated: boolean;
  expiresAt: number | null;
  error: string | null;
}

export interface McodeOAuthStatus {
  accounts: McodeOAuthAccount[];
  currentAccountId: string | null;
}

export interface McodeLoginStart {
  flowId: string;
  userCode: string;
  verificationUri: string;
  expiresIn: number;
  interval: number;
}

export const mcodeOAuthApi = {
  status: () => invoke<McodeOAuthStatus>("mcode_oauth_status"),
  start: (region: McodeRegion, accountId?: string) =>
    invoke<McodeLoginStart>("mcode_oauth_start", {
      region,
      accountId: accountId ?? null,
    }),
  poll: (flowId: string) =>
    invoke<{
      authenticated: boolean;
      interval: number;
      ended: "expired" | "denied" | "cancelled" | null;
    }>("mcode_oauth_poll", { flowId }),
  cancel: (flowId: string) =>
    invoke<boolean>("mcode_oauth_cancel", { flowId }),
  remove: (accountId: string) =>
    invoke<void>("mcode_oauth_remove", { accountId }),
  quota: (accountId: string) =>
    invoke<SubscriptionQuota>("mcode_oauth_quota", { accountId }),
  useAccount: (accountId: string) =>
    invoke<void>("mcode_oauth_use_account", { accountId }),
};
