import { invoke } from "@tauri-apps/api/core";
import type {
  HuaweiCodeAgentAuthStatus,
  HuaweiCodeAgentAccount,
  FetchedModel,
} from "./types";

export const huaweiCodeAgent2Api = {
  async startLogin(
    user: string,
    password: string,
  ): Promise<HuaweiCodeAgentAuthStatus> {
    return await invoke("huawei_codeagent_2_start_login", { user, password });
  },

  async getStatus(): Promise<HuaweiCodeAgentAuthStatus> {
    return await invoke("huawei_codeagent_2_get_status");
  },

  async logout(): Promise<void> {
    await invoke("huawei_codeagent_2_logout");
  },

  async refreshToken(accountId: string): Promise<HuaweiCodeAgentAuthStatus> {
    return await invoke("huawei_codeagent_2_refresh_token", { accountId });
  },

  async listAccounts(): Promise<HuaweiCodeAgentAccount[]> {
    return await invoke("huawei_codeagent_2_list_accounts");
  },

  async removeAccount(accountId: string): Promise<void> {
    await invoke("huawei_codeagent_2_remove_account", { accountId });
  },

  async setDefaultAccount(accountId: string): Promise<void> {
    await invoke("huawei_codeagent_2_set_default_account", { accountId });
  },
};

export async function fetchHuaweiCodeAgent2Models(
  accountId?: string | null,
): Promise<FetchedModel[]> {
  return invoke("huawei_codeagent_2_fetch_models", { accountId });
}

export interface HuaweiQuota {
  type: string;
  used_quota: number | string;
  total_quota: number | string;
  reset_at?: string;
  remaining_quota?: number | string;
  period_start?: string;
  period_end?: string;
}
export interface HuaweiQuotaReport {
  quotas: HuaweiQuota[];
  external_quotas?: HuaweiQuota[];
  internal_quotas?: HuaweiQuota[];
  message_cn?: string;
  message_ch?: string;
  message_en?: string;
}
export function fetchHuaweiQuota(
  accountId?: string | null,
): Promise<HuaweiQuotaReport> {
  return invoke("huawei_codeagent_2_fetch_quota", { accountId });
}

export function fetchHuaweiModelQuota(
  accountId: string | null,
  modelId: string,
): Promise<HuaweiQuotaReport> {
  return invoke("huawei_codeagent_2_fetch_quota", { accountId, modelId });
}
