/// <reference types="vite/client" />

import { Provider, Settings } from "./types";
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
      getConfigDir: (app?: AppType) => Promise<string>;
      selectConfigDirectory: (defaultPath?: string) => Promise<string | null>;
      openConfigFolder: (app?: AppType) => Promise<void>;
      openExternal: (url: string) => Promise<void>;
      updateTrayMenu: () => Promise<boolean>;
      onProviderSwitched: (
        callback: (data: { appType: string; providerId: string }) => void,
      ) => Promise<UnlistenFn>;
      getSettings: () => Promise<Settings>;
      saveSettings: (settings: Settings) => Promise<boolean>;
      checkForUpdates: () => Promise<void>;
      isPortable: () => Promise<boolean>;
      getAppConfigPath: () => Promise<string>;
      openAppConfigFolder: () => Promise<void>;
      // VS Code settings.json 能力
      getVSCodeSettingsStatus: () => Promise<ConfigStatus>;
      readVSCodeSettings: () => Promise<string>;
      writeVSCodeSettings: (content: string) => Promise<boolean>;
      // 云同步功能
      cloudSync: {
        validateGitHubToken: (githubToken: string) => Promise<{ valid: boolean; user?: any; message: string }>;
        configure: (config: {
          githubToken: string;
          gistUrl?: string;
          encryptionPassword: string;
          autoSyncEnabled: boolean;
          syncOnStartup: boolean;
        }) => Promise<{ success: boolean; message: string }>;
        getSettings: (encryptionPassword: string) => Promise<{
          configured: boolean;
          gistUrl?: string;
          enabled: boolean;
          syncMode: string;
          lastSyncTimestamp?: string;
          hasToken?: boolean;
        }>;
        syncToCloud: (encryptionPassword: string, forceOverwrite?: boolean) => Promise<{
          success: boolean;
          gistUrl: string;
          backupId: string;
          message: string;
        }>;
        syncFromCloud: (gistUrl: string, encryptionPassword: string, autoApply?: boolean) => Promise<{
          success: boolean;
          applied: boolean;
          configuration?: string;
          backupId?: string;
          message: string;
        }>;
      };
    };
    platform: {
      isMac: boolean;
    };
    __TAURI__?: any;
  }
}

export {};
