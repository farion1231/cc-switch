import { invoke } from "@tauri-apps/api/core";
import type { ProviderMeta } from "@/types";

export type DimSite = "cn" | "global";
export interface DimAccount {
  id: string;
  site: DimSite;
  login: string;
  authenticated_at: number;
  requires_reauth: boolean;
}
export interface DimStatus {
  accounts: DimAccount[];
  default_account_id: string | null;
}
export interface DimLoginFlow {
  flow_id: string;
  auth_url: string;
  expires_in: number;
  interval: number;
}
export interface DimModel {
  id: string;
  name: string;
  api_formats: NonNullable<ProviderMeta["apiFormat"]>[];
  efforts: string[];
  zdr: boolean;
  context_window: number | null;
  max_output_tokens: number | null;
}
export interface DimUsage {
  plan_name: string | null;
  remaining_units: number | null;
  windows: Array<{
    name: string;
    used: number;
    total: number;
    unit: string;
    resets_at: string | null;
  }>;
  subscription_url: string;
  pricing_url: string;
}
export const dimOAuthApi = {
  status: () => invoke<DimStatus>("get_dim_oauth_status"),
  start: (site: DimSite) =>
    invoke<DimLoginFlow>("start_dim_oauth_login", { site }),
  poll: (flowId: string) =>
    invoke<DimAccount | null>("poll_dim_oauth_login", { flowId }),
  cancel: (flowId: string) =>
    invoke<void>("cancel_dim_oauth_login", { flowId }),
  remove: (accountId: string) =>
    invoke<void>("remove_dim_oauth_account", { accountId }),
  setDefault: (accountId: string) =>
    invoke<void>("set_dim_oauth_default_account", { accountId }),
  models: (accountId: string) =>
    invoke<DimModel[]>("get_dim_oauth_models", { accountId }),
  usage: (accountId: string) =>
    invoke<DimUsage>("get_dim_oauth_usage", { accountId }),
};
