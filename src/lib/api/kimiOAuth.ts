import { invoke } from "@tauri-apps/api/core";
import type { SubscriptionQuota } from "@/types/subscription";
import type { KimiProviderConfig } from "@/config/kimiProviderPresets";

export type KimiRegion = "mainland-cn" | "global";

export interface KimiOAuthAccount {
  id: string;
  name: string;
  nickname: string | null;
  avatarUrl: string | null;
  /** Kimi's own default login: web search and fetch always use it. */
  primary: boolean;
  authenticated: boolean;
  region: KimiRegion;
  expiresAt: number | null;
  error: string | null;
}

export interface KimiOAuthStatus {
  accounts: KimiOAuthAccount[];
  currentAccountId: string | null;
  /** Kimi 自己的登录还在，只是被移出了列表 */
  primaryHidden: boolean;
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
  /** Separate accounts are deleted; Kimi's own login is only hidden. */
  remove: (accountId: string) =>
    invoke<void>("kimi_oauth_remove", { accountId }),
  quota: (accountId: string) =>
    invoke<SubscriptionQuota>("kimi_oauth_quota", { accountId }),
  /** 账号当前的官方模型目录（键和内容与登录时写入的一致），只读不存 */
  models: (accountId: string) =>
    invoke<KimiProviderConfig["models"]>("kimi_oauth_models", { accountId }),
  useAccount: (accountId: string) =>
    invoke<void>("kimi_oauth_use_account", { accountId }),
};
