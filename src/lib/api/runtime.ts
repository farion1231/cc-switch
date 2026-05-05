import { getEffectiveRemoteBackendConfig, invoke } from "./transport";
import { getClientOs, isCliWebUi } from "@/lib/platform";

export type RuntimeOs = "windows" | "macos" | "linux" | "unknown";
export type ClientShell = "desktop" | "browser";

export interface RuntimeInfo {
  client: {
    shell: ClientShell;
    os: RuntimeOs;
  };
  backend: {
    os: RuntimeOs;
    headless: boolean;
    remote: boolean;
    capabilities: {
      readConfig: boolean;
      writeConfig: boolean;
      openLocalFolder: boolean;
      pickDirectory: boolean;
      serverDirectoryBrowse: boolean;
      appConfigDirOverride: boolean;
      saveFileDialog: boolean;
      openFileDialog: boolean;
      launchInteractiveTerminal: boolean;
      launchBackgroundProcess: boolean;
      autoLaunch: boolean;
      toolVersionCheck: boolean;
      windowControls: boolean;
      dragRegion: boolean;
      tray: boolean;
    };
  };
  relation: {
    coLocated: boolean;
  };
}

let runtimeInfoPromise: Promise<RuntimeInfo> | null = null;

export const runtimeApi = {
  async get(): Promise<RuntimeInfo> {
    const info = await invoke<RuntimeInfo>("get_runtime_info");
    const clientShell = isCliWebUi() ? "browser" : "desktop";
    const remoteBackend = await getEffectiveRemoteBackendConfig();
    return {
      ...info,
      client: {
        shell: clientShell,
        os: getClientOs(),
      },
      relation: {
        coLocated:
          info.relation.coLocated &&
          clientShell === "desktop" &&
          !remoteBackend,
      },
    };
  },

  async getCached(): Promise<RuntimeInfo> {
    runtimeInfoPromise ??= runtimeApi.get();
    return await runtimeInfoPromise;
  },

  clearCache(): void {
    runtimeInfoPromise = null;
  },
};
