import { invoke } from "@tauri-apps/api/core";

export interface DshAccountProfile {
  id: string | null;
  name: string | null;
  /** 平台返回的脱敏手机号或邮箱 */
  contact: string | null;
  avatarUrl: string | null;
}

export interface DshWallet {
  currency: "CNY" | "USD" | string;
  /** 十进制字符串，原样来自平台 */
  balance: string;
}

/**
 * DeepSeek 账号（OAuth）登录：存在 `$DSH_HOME/.credentials.yaml`，DeepSeek Harness 的
 * Web 和 Desktop 共用同一份，任一端登录 / 退出两端同时生效。
 */
export interface DshAccountStatus {
  signedIn: boolean;
  /** 平台拒绝了保存的登录（已过期），DeepSeek Harness 下次请求账号时会清掉 */
  expired: boolean;
  issuer: string | null;
  profile: DshAccountProfile | null;
  /** 充值余额 */
  wallets: DshWallet[];
  /** 赠金余额（只列大于 0 的） */
  bonusWallets: DshWallet[];
  usageUrl: string | null;
  topUpUrl: string | null;
  /** 资料或余额没查到；登录本身还在 */
  error: string | null;
}

export interface DshLoginStart {
  flowId: string;
  /** Platform 的授权页（`https://platform.deepseek.com/dsh/authorize?...`） */
  authUrl: string;
  expiresIn: number;
}

export const dshApi = {
  accountStatus: () => invoke<DshAccountStatus>("dsh_account_status"),
  /** Web 和 Desktop 同时退出 */
  accountSignOut: () => invoke<void>("dsh_account_sign_out"),
  /** 和 DeepSeek Harness Web 同一套浏览器登录；登录写进 Web 与 Desktop 共用的凭据文件 */
  loginStart: () => invoke<DshLoginStart>("dsh_account_login_start"),
  /** 还在等浏览器时是 false，登录已保存时是 true */
  loginPoll: (flowId: string) =>
    invoke<boolean>("dsh_account_login_poll", { flowId }),
  loginCancel: (flowId: string) =>
    invoke<void>("dsh_account_login_cancel", { flowId }),
};
