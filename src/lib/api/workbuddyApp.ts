import { invoke } from "@tauri-apps/api/core";

/** 「应用」页 WorkBuddy 行：本机装没装、版本、在不在运行。 */
export interface WorkBuddyAppStatus {
  installed: boolean;
  /** macOS 读 Info.plist；其他平台读不到时为 null */
  version: string | null;
  path: string | null;
  running: boolean;
  /** 这个平台能不能在这里请 WorkBuddy 退出（目前只有 macOS） */
  quitSupported: boolean;
}

export interface WorkBuddyQuitOutcome {
  /** 等完之后仍在运行：WorkBuddy 拒绝了退出或还没退完，需要用户手动退出 */
  stillRunning: boolean;
}

export async function getStatus(): Promise<WorkBuddyAppStatus> {
  return invoke<WorkBuddyAppStatus>("workbuddy_app_status");
}

/** 启动 WorkBuddy（已在运行就切到前台）。 */
export async function start(): Promise<void> {
  return invoke("workbuddy_account_open_app");
}

/** 请 WorkBuddy 正常退出（不强制结束）并等它退完。 */
export async function quit(): Promise<WorkBuddyQuitOutcome> {
  return invoke<WorkBuddyQuitOutcome>("workbuddy_app_quit");
}

export const workbuddyAppApi = { getStatus, start, quit };
