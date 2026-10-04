import { invoke } from "@tauri-apps/api/core";
import type { AppId } from "./types";
import type { UsageResult } from "@/types";
import type { SubscriptionQuota } from "@/types/subscription";
import type { ManagedAuthAccount, ManagedAuthProvider } from "./auth";

/** 托盘面板里一个应用：在用的那家 + 额度快照（后端 `tray::TrayPanelApp`） */
export interface TrayPanelApp {
  appType: AppId;
  appName: string;
  providerId: string;
  providerName: string;
  usageKind: "subscription" | "managedCodex" | "script" | null;
  accountId: string | null;
  tokenPlan: boolean;
  subscription: SubscriptionQuota | null;
  script: UsageResult | null;
}

/** 授权中心里一个账号和后台上次查到的额度（Copilot 也折成 SubscriptionQuota） */
export interface TrayPanelAccount {
  provider: ManagedAuthProvider;
  account: ManagedAuthAccount;
  /** 需要重新登录的账号不查，为空 */
  quota: SubscriptionQuota | null;
}

export interface TrayPanelSnapshot {
  apps: TrayPanelApp[];
  accounts: TrayPanelAccount[];
  /** 上次查询完成的时间（毫秒）；还没查过时为空 */
  refreshedAt: number | null;
}

export const trayPanelApi = {
  /** 读缓存，不发请求：额度由后台每 5 分钟查一次 */
  getSnapshot: () => invoke<TrayPanelSnapshot>("get_tray_panel_snapshot"),
  /** 手动刷新：立刻查一遍再返回 */
  refresh: () => invoke<TrayPanelSnapshot>("refresh_tray_panel"),
  hide: () => invoke<void>("tray_panel_hide"),
  setHeight: (height: number) =>
    invoke<void>("tray_panel_set_height", { height }),
  openMain: () => invoke<void>("tray_panel_open_main"),
};

/** 后端每次弹出面板时发这个事件 */
export const TRAY_PANEL_SHOWN_EVENT = "tray-panel-shown";
/** 后台查完额度发这个事件 */
export const TRAY_PANEL_UPDATED_EVENT = "tray-panel-updated";
