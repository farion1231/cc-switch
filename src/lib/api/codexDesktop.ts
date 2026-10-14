import { invoke } from "@tauri-apps/api/core";

export interface DesktopRestartOutcome {
  action: "started" | "restarted";
  desktop_pid: number;
}

export interface DesktopStatus {
  supported: boolean;
  installed: boolean;
  running: boolean;
}

export function getCodexDesktopStatus(): Promise<DesktopStatus> {
  return invoke("get_codex_desktop_status");
}

export function restartCodexDesktop(): Promise<DesktopRestartOutcome> {
  return invoke("restart_codex_desktop");
}
