import { invoke } from "@tauri-apps/api/core";
import type { Settings } from "@/types";
import type { AppId } from "./types";

export interface ConfigTransferResult {
  success: boolean;
  message: string;
  filePath?: string;
  backupId?: string;
}

export const settingsApi = {
  async get(): Promise<Settings> {
    return await invoke("get_settings");
  },

  async save(settings: Settings): Promise<boolean> {
    return await invoke("save_settings", { settings });
  },

  async restart(): Promise<boolean> {
    return await invoke("restart_app");
  },

  async checkUpdates(): Promise<void> {
    await invoke("check_for_updates");
  },

  async isPortable(): Promise<boolean> {
    return await invoke("is_portable_mode");
  },

  async getConfigDir(appId: AppId): Promise<string> {
    return await invoke("get_config_dir", { app: appId });
  },

  async openConfigFolder(appId: AppId): Promise<void> {
    await invoke("open_config_folder", { app: appId });
  },

  async selectConfigDirectory(defaultPath?: string): Promise<string | null> {
    return await invoke("pick_directory", { defaultPath });
  },

  async getClaudeCodeConfigPath(): Promise<string> {
    return await invoke("get_claude_code_config_path");
  },

  async getAppConfigPath(): Promise<string> {
    return await invoke("get_app_config_path");
  },

  async openAppConfigFolder(): Promise<void> {
    await invoke("open_app_config_folder");
  },

  async getAppConfigDirOverride(): Promise<string | null> {
    return await invoke("get_app_config_dir_override");
  },

  async setAppConfigDirOverride(path: string | null): Promise<boolean> {
    return await invoke("set_app_config_dir_override", { path });
  },

  async applyClaudePluginConfig(options: {
    official: boolean;
  }): Promise<boolean> {
    const { official } = options;
    return await invoke("apply_claude_plugin_config", { official });
  },

  async applyClaudeOnboardingSkip(): Promise<boolean> {
    return await invoke("apply_claude_onboarding_skip");
  },

  async clearClaudeOnboardingSkip(): Promise<boolean> {
    return await invoke("clear_claude_onboarding_skip");
  },

  async saveFileDialog(defaultName: string): Promise<string | null> {
    return await invoke("save_file_dialog", { defaultName });
  },

  async openFileDialog(): Promise<string | null> {
    return await invoke("open_file_dialog");
  },

  async exportConfigToFile(filePath: string): Promise<ConfigTransferResult> {
    return await invoke("export_config_to_file", { filePath });
  },

  async importConfigFromFile(filePath: string): Promise<ConfigTransferResult> {
    return await invoke("import_config_from_file", { filePath });
  },

  async syncCurrentProvidersLive(): Promise<void> {
    const result = (await invoke("sync_current_providers_live")) as {
      success?: boolean;
      message?: string;
    };
    if (!result?.success) {
      throw new Error(result?.message || "Sync current providers failed");
    }
  },

  async openExternal(url: string): Promise<void> {
    try {
      const u = new URL(url);
      const scheme = u.protocol.replace(":", "").toLowerCase();
      if (scheme !== "http" && scheme !== "https") {
        throw new Error("Unsupported URL scheme");
      }
    } catch {
      throw new Error("Invalid URL");
    }
    await invoke("open_external", { url });
  },

  async setAutoLaunch(enabled: boolean): Promise<boolean> {
    return await invoke("set_auto_launch", { enabled });
  },

  async getAutoLaunchStatus(): Promise<boolean> {
    return await invoke("get_auto_launch_status");
  },

  async getToolVersions(): Promise<
    Array<{
      name: string;
      version: string | null;
      latest_version: string | null;
      error: string | null;
    }>
  > {
    return await invoke("get_tool_versions");
  },

  async getRectifierConfig(): Promise<RectifierConfig> {
    return await invoke("get_rectifier_config");
  },

  async setRectifierConfig(config: RectifierConfig): Promise<boolean> {
    return await invoke("set_rectifier_config", { config });
  },
};

export interface RectifierConfig {
  enabled: boolean;
  requestThinkingSignature: boolean;
}
