import { invoke } from "@tauri-apps/api/core";

/** 支持插件的应用：Claude Code 和 Codex 各自有插件市场。 */
export type PluginApp = "claude" | "codex";

export const PLUGIN_APPS: PluginApp[] = ["claude", "codex"];

/** 工具没装时后端返回的错误码。 */
export const PLUGIN_CLI_NOT_INSTALLED = "PLUGIN_CLI_NOT_INSTALLED";

export interface PluginEntry {
  /** `name@marketplace` */
  id: string;
  name: string;
  marketplace: string;
  displayName?: string | null;
  version?: string | null;
  description?: string | null;
  category?: string | null;
  installed: boolean;
  enabled: boolean;
  /** Claude Code 的安装范围（user / project / local / managed） */
  scope?: string | null;
  /** CC Switch 能否启停、卸载（Claude Code 只管 user 范围） */
  manageable: boolean;
  /** 市场是否允许安装 */
  installable: boolean;
  installPath?: string | null;
  updatedAt?: string | null;
}

export interface PluginMarketplace {
  name: string;
  source?: string | null;
  location?: string | null;
  removable: boolean;
}

export interface PluginActionOutcome {
  changed: boolean;
  message?: string | null;
}

export const pluginsApi = {
  /** 已安装的插件（便宜，每次操作后都会重读） */
  listInstalled: (app: PluginApp) =>
    invoke<PluginEntry[]>("list_installed_plugins", { app }),
  /** 所有市场里的插件（含已安装的；大，只在市场变动后重读） */
  listCatalog: (app: PluginApp) =>
    invoke<PluginEntry[]>("list_plugin_catalog", { app }),
  listMarketplaces: (app: PluginApp) =>
    invoke<PluginMarketplace[]>("list_plugin_marketplaces", { app }),
  install: (app: PluginApp, id: string) =>
    invoke<PluginActionOutcome>("install_plugin", { app, id }),
  uninstall: (app: PluginApp, id: string) =>
    invoke<PluginActionOutcome>("uninstall_plugin", { app, id }),
  setEnabled: (app: PluginApp, id: string, enabled: boolean) =>
    invoke<PluginActionOutcome>("set_plugin_enabled", { app, id, enabled }),
  update: (app: PluginApp, id: string) =>
    invoke<PluginActionOutcome>("update_plugin", { app, id }),
  addMarketplace: (app: PluginApp, source: string) =>
    invoke<PluginActionOutcome>("add_plugin_marketplace", { app, source }),
  removeMarketplace: (app: PluginApp, name: string) =>
    invoke<PluginActionOutcome>("remove_plugin_marketplace", { app, name }),
  refreshMarketplaces: (app: PluginApp) =>
    invoke<PluginActionOutcome>("refresh_plugin_marketplaces", { app }),
};
