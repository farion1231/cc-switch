/// <reference types="vite/client" />

import { Provider } from "./types";
import { AppType } from "./lib/tauri-api";
import type { UnlistenFn } from "@tauri-apps/api/event";

interface ImportResult {
  success: boolean;
  message?: string;
}

interface ConfigStatus {
  exists: boolean;
  path: string;
  error?: string;
}

declare global {
  interface Window {
    api: {
      getProviders: (app?: AppType) => Promise<Record<string, Provider>>;
      getCurrentProvider: (app?: AppType) => Promise<string>;
      addProvider: (provider: Provider, app?: AppType) => Promise<boolean>;
      deleteProvider: (id: string, app?: AppType) => Promise<boolean>;
      updateProvider: (provider: Provider, app?: AppType) => Promise<boolean>;
      switchProvider: (providerId: string, app?: AppType) => Promise<boolean>;
      importCurrentConfigAsDefault: (app?: AppType) => Promise<ImportResult>;
      getClaudeCodeConfigPath: () => Promise<string>;
      getClaudeConfigStatus: () => Promise<ConfigStatus>;
      getConfigStatus: (app?: AppType) => Promise<ConfigStatus>;
      selectConfigFile: () => Promise<string | null>;
      openConfigFolder: (app?: AppType) => Promise<void>;
      openExternal: (url: string) => Promise<void>;
      updateTrayMenu: () => Promise<boolean>;
      onProviderSwitched: (
        callback: (data: { appType: string; providerId: string }) => void
      ) => Promise<UnlistenFn>;
    };
    platform: {
      isMac: boolean;
    };
    __TAURI__?: any;
  }
}

export {};
