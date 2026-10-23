import { invoke } from "@tauri-apps/api/core";

/** Hermes 按 profile 独立路由的状态（后端 hermes_profile::ProfileStatus）。 */
export interface HermesProfileStatus {
  name: string;
  /** 独立代理模式开着。 */
  independent: boolean;
  /** 客户端文件当前接在独立命名空间上。 */
  attached: boolean;
  routeProviderId: string | null;
  routeProviderName: string | null;
  autoFailoverEnabled: boolean;
  loadBalanceMode: string;
}

export const hermesProfileApi = {
  list: () => invoke<HermesProfileStatus[]>("list_hermes_profiles"),

  enter: (params: {
    profile: string;
    routeProviderId: string;
    autoFailover?: boolean;
    loadBalanceMode?: "off" | "session";
  }) =>
    invoke<void>("enter_hermes_profile_proxy", {
      profile: params.profile,
      routeProviderId: params.routeProviderId,
      autoFailover: params.autoFailover ?? false,
      loadBalanceMode: params.loadBalanceMode ?? "off",
    }),

  exit: (profile: string) =>
    invoke<void>("exit_hermes_profile_proxy", { profile }),

  switchRoute: (profile: string, routeProviderId: string) =>
    invoke<boolean>("switch_hermes_profile_route", {
      profile,
      routeProviderId,
    }),
};
