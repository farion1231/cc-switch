import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  pluginsApi,
  type PluginApp,
  type PluginScope,
} from "@/lib/api/plugins";

export const pluginKeys = {
  all: ["plugins"] as const,
  statuses: ["plugins", "statuses"] as const,
  list: (app: PluginApp, includeAvailable: boolean) =>
    ["plugins", "list", app, includeAvailable] as const,
  marketplaces: (app: PluginApp) => ["plugins", "marketplaces", app] as const,
};

export function usePluginStatuses() {
  return useQuery({
    queryKey: pluginKeys.statuses,
    queryFn: pluginsApi.getClientStatuses,
  });
}

export function usePlugins(
  app: PluginApp,
  includeAvailable: boolean,
  enabled = true,
) {
  return useQuery({
    queryKey: pluginKeys.list(app, includeAvailable),
    queryFn: () => pluginsApi.list(app, includeAvailable),
    enabled,
  });
}

export function usePluginMarketplaces(app: PluginApp, enabled = true) {
  return useQuery({
    queryKey: pluginKeys.marketplaces(app),
    queryFn: () => pluginsApi.listMarketplaces(app),
    enabled,
  });
}

export type PluginMutation =
  | { action: "addMarketplace"; app: PluginApp; source: string }
  | { action: "refreshMarketplace"; app: PluginApp; name: string }
  | { action: "removeMarketplace"; app: PluginApp; name: string }
  | {
      action: "install";
      app: PluginApp;
      pluginId: string;
      scope?: PluginScope;
      projectPath?: string;
    }
  | {
      action: "update";
      app: PluginApp;
      pluginId: string;
      scope?: PluginScope;
      projectPath?: string;
    }
  | {
      action: "setEnabled";
      app: PluginApp;
      pluginId: string;
      enabled: boolean;
      scope?: PluginScope;
      projectPath?: string;
    }
  | {
      action: "uninstall";
      app: PluginApp;
      pluginId: string;
      scope?: PluginScope;
      projectPath?: string;
    };

export function usePluginMutation() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (request: PluginMutation) => {
      switch (request.action) {
        case "addMarketplace":
          return pluginsApi.addMarketplace(request.app, request.source);
        case "refreshMarketplace":
          return pluginsApi.refreshMarketplace(request.app, request.name);
        case "removeMarketplace":
          return pluginsApi.removeMarketplace(request.app, request.name);
        case "install":
          return pluginsApi.install(
            request.app,
            request.pluginId,
            request.scope,
            request.projectPath,
          );
        case "update":
          return pluginsApi.update(
            request.app,
            request.pluginId,
            request.scope,
            request.projectPath,
          );
        case "setEnabled":
          return pluginsApi.setEnabled(
            request.app,
            request.pluginId,
            request.enabled,
            request.scope,
            request.projectPath,
          );
        case "uninstall":
          return pluginsApi.uninstall(
            request.app,
            request.pluginId,
            request.scope,
            request.projectPath,
          );
      }
    },
    onSuccess: () =>
      queryClient.invalidateQueries({ queryKey: pluginKeys.all }),
  });
}
