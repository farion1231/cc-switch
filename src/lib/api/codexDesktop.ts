import { invoke } from "@tauri-apps/api/core";

export interface DesktopRestartOutcome {
  action: "started" | "restarted";
  desktop_pid: number;
}

export function restartCodexDesktop(): Promise<DesktopRestartOutcome> {
  return invoke("restart_codex_desktop");
}
