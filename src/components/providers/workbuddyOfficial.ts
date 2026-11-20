import { resolveManagedAccountId } from "@/lib/authBinding";
import { WORKBUDDY_ACCOUNT_AUTH_PROVIDER } from "@/config/workbuddyProviderPresets";
import type { Provider } from "@/types";

/** 和授权中心的 WorkBuddy 分组共用缓存 */
export const WORKBUDDY_STATUS_KEY = [
  "managed-auth-status",
  WORKBUDDY_ACCOUNT_AUTH_PROVIDER,
];
export const WORKBUDDY_LIVE_KEY = ["workbuddy-account-live"];

/** WorkBuddy 官方登录卡：只绑定授权中心的一个账号，不写进 models.json。 */
export function isWorkBuddyOfficial(provider: Provider): boolean {
  return provider.category === "official";
}

/** 官方卡绑定的 WorkBuddy 账号 id；没绑时为 null。 */
export function boundWorkBuddyAccountId(provider: Provider): string | null {
  return (
    resolveManagedAccountId(
      provider.meta,
      WORKBUDDY_ACCOUNT_AUTH_PROVIDER,
    )?.trim() || null
  );
}

/** 供应商列表上官方卡要的状态：已收录的账号、WorkBuddy 当前登录、是否在运行。 */
export interface WorkBuddyOfficialState {
  /** 已收录账号的 id；还没读到时为 null */
  accountIds: ReadonlySet<string> | null;
  currentAccountId: string | null;
  running: boolean;
  switching: boolean;
}
