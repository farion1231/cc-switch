import { invoke } from "@tauri-apps/api/core";
import type { AppId } from "./types";
import type { UsageResult } from "@/types";
import type { SubscriptionQuota } from "@/types/subscription";

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

export const trayPanelApi = {
  /** 读缓存，不发请求 */
  getApps: () => invoke<TrayPanelApp[]>("get_tray_panel_apps"),
  /** 刷新在用那家的额度（后端 10 秒节流）后返回 */
  refreshApps: () => invoke<TrayPanelApp[]>("refresh_tray_panel_apps"),
  hide: () => invoke<void>("tray_panel_hide"),
  setHeight: (height: number) =>
    invoke<void>("tray_panel_set_height", { height }),
  openMain: () => invoke<void>("tray_panel_open_main"),
};

/** 后端每次弹出面板时发这个事件 */
export const TRAY_PANEL_SHOWN_EVENT = "tray-panel-shown";
