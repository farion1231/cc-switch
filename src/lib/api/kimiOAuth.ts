import { invoke } from "@tauri-apps/api/core";

export type KimiRegion = "mainland-cn" | "global";

export interface KimiOAuthStatus {
  authenticated: boolean;
  region: KimiRegion;
  expiresAt: number | null;
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
  start: (region: KimiRegion) =>
    invoke<KimiLoginStart>("kimi_oauth_start", { region }),
  poll: (flowId: string) =>
    invoke<{
      authenticated: boolean;
      interval: number;
      ended: "expired" | "denied" | "cancelled" | null;
    }>("kimi_oauth_poll", { flowId }),
  cancel: (flowId: string) => invoke<boolean>("kimi_oauth_cancel", { flowId }),
  logout: () => invoke<void>("kimi_oauth_logout"),
  useAccount: () => invoke<void>("kimi_oauth_use_account"),
};
