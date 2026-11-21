import { invoke } from "@tauri-apps/api/core";
import { useQuery } from "@tanstack/react-query";

export interface CustomApp {
  id: string;
  name: string;
  token?: string;
}

export interface CustomProvider {
  name: string;
  baseUrl: string;
  model: string;
  apiKey?: string;
}

export const customAppsKey = ["custom-apps"] as const;
export const customProvidersKey = (appId: string) =>
  ["custom-app-providers", appId] as const;

export const customAppsApi = {
  list: () => invoke<CustomApp[]>("list_custom_apps"),
  create: (name: string) => invoke<CustomApp>("create_custom_app", { name }),
  rename: (appId: string, name: string) =>
    invoke<void>("rename_custom_app", { appId, name }),
  delete: (appId: string) => invoke<void>("delete_custom_app", { appId }),
  updateProvider: (
    appId: string,
    providerId: string,
    provider: CustomProvider,
  ) => invoke<void>("update_custom_provider", { appId, providerId, provider }),
  deleteProvider: (appId: string, providerId: string) =>
    invoke<void>("delete_custom_provider", { appId, providerId }),
  rotateToken: (appId: string) =>
    invoke<string>("rotate_custom_app_token", { appId }),
  providers: (appId: string) =>
    invoke<{
      providers: { id: string; name: string; model: string; baseUrl: string }[];
      selected: string | null;
    }>("list_custom_providers", { appId }),
  addProvider: (appId: string, provider: CustomProvider) =>
    invoke<void>("add_custom_provider", { appId, provider }),
  selectProvider: (appId: string, providerId: string) =>
    invoke<void>("select_custom_provider", { appId, providerId }),
};

export function useCustomApps() {
  return useQuery({ queryKey: customAppsKey, queryFn: customAppsApi.list });
}
