import { invoke } from "@tauri-apps/api/core";

export type PluginApp = "codex" | "claude";
export type PluginScope = "user" | "project" | "local";

export interface PluginActions {
  install: boolean;
  update: boolean;
  enable: boolean;
  disable: boolean;
  uninstall: boolean;
}

export interface PluginClientStatus {
  app: PluginApp;
  available: boolean;
  version?: string;
  error?: string;
  supportedActions: PluginActions;
}

export interface UnifiedPlugin {
  pluginId: string;
  name: string;
  description?: string;
  version?: string;
  app: PluginApp;
  marketplaceName: string;
  installed: boolean;
  enabled: boolean;
  scope?: PluginScope;
  projectPath?: string;
  source?: string;
  supportedActions: PluginActions;
}

export interface PluginMarketplace {
  name: string;
  app: PluginApp;
  sourceType?: string;
  source?: string;
  root?: string;
}

export interface PluginActionResult {
  success: boolean;
  requiresRestart: boolean;
  commandSummary: string;
}

export const pluginsApi = {
  getClientStatuses: () =>
    invoke<PluginClientStatus[]>("get_plugin_client_statuses"),
  list: (app: PluginApp, includeAvailable: boolean) =>
    invoke<UnifiedPlugin[]>("list_plugins", { app, includeAvailable }),
  listMarketplaces: (app: PluginApp) =>
    invoke<PluginMarketplace[]>("list_plugin_marketplaces", { app }),
  addMarketplace: (app: PluginApp, source: string) =>
    invoke<PluginActionResult>("add_plugin_marketplace", { app, source }),
  refreshMarketplace: (app: PluginApp, name: string) =>
    invoke<PluginActionResult>("refresh_plugin_marketplace", { app, name }),
  removeMarketplace: (app: PluginApp, name: string) =>
    invoke<PluginActionResult>("remove_plugin_marketplace", { app, name }),
  install: (
    app: PluginApp,
    pluginId: string,
    scope?: PluginScope,
    projectPath?: string,
  ) =>
    invoke<PluginActionResult>("install_plugin", {
      app,
      pluginId,
      scope,
      projectPath,
    }),
  update: (
    app: PluginApp,
    pluginId: string,
    scope?: PluginScope,
    projectPath?: string,
  ) =>
    invoke<PluginActionResult>("update_plugin", {
      app,
      pluginId,
      scope,
      projectPath,
    }),
  setEnabled: (
    app: PluginApp,
    pluginId: string,
    enabled: boolean,
    scope?: PluginScope,
    projectPath?: string,
  ) =>
    invoke<PluginActionResult>("set_plugin_enabled", {
      app,
      pluginId,
      enabled,
      scope,
      projectPath,
    }),
  uninstall: (
    app: PluginApp,
    pluginId: string,
    scope?: PluginScope,
    projectPath?: string,
  ) =>
    invoke<PluginActionResult>("uninstall_plugin", {
      app,
      pluginId,
      scope,
      projectPath,
    }),
};
