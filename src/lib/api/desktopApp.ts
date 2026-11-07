import { invoke } from "@tauri-apps/api/core";
import type { AppId } from "./types";

export interface DesktopAppStatus {
  supported: boolean;
  installed: boolean;
  running: boolean;
}

export const desktopAppApi = {
  async getStatus(app: AppId): Promise<DesktopAppStatus> {
    return await invoke("get_desktop_app_status", { app });
  },

  async launch(app: AppId): Promise<void> {
    await invoke("launch_desktop_app", { app });
  },

  async restart(app: AppId): Promise<void> {
    await invoke("restart_desktop_app", { app });
  },
};
