import { invoke } from "@tauri-apps/api/core";

export type KimiRegion = "mainland-cn" | "global";

export interface KimiOAuthAccount {
  id: string;
  name: string;
  authenticated: boolean;
  region: KimiRegion;
  expiresAt: number | null;
  error: string | null;
}

export interface KimiOAuthStatus {
  accounts: KimiOAuthAccount[];
  currentAccountId: string | null;
}

export interface KimiLoginStart {
  flowId: string;
  userCode: string;
  verificationUri: string;
  expiresIn: number;
  interval: number;
}

export const kimiOAuthApi = {
  status: () => invoke<KimiOAuthStatus>("kimi_oauth_status"),
  start: (region: KimiRegion, accountId?: string) =>
    invoke<KimiLoginStart>("kimi_oauth_start", {
      region,
      accountId: accountId ?? null,
    }),
  poll: (flowId: string) =>
    invoke<{
      authenticated: boolean;
      interval: number;
      ended: "expired" | "denied" | "cancelled" | null;
    }>("kimi_oauth_poll", { flowId }),
  cancel: (flowId: string) => invoke<boolean>("kimi_oauth_cancel", { flowId }),
  logout: (accountId: string) =>
    invoke<void>("kimi_oauth_logout", { accountId }),
  useAccount: (accountId: string) =>
    invoke<void>("kimi_oauth_use_account", { accountId }),
};
