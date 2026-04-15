import { invoke } from "@tauri-apps/api/core";
import type {
  CredentialScanStatus,
  SubscriptionQuota,
} from "@/types/subscription";

export const subscriptionApi = {
  getQuota: (tool: string): Promise<SubscriptionQuota> =>
    invoke("get_subscription_quota", { tool }),
  getCredentialScanStatus: (tool: string): Promise<CredentialScanStatus> =>
    invoke("get_credential_scan_status", { tool }),
  launchGeminiOauthLogin: (): Promise<boolean> =>
    invoke("launch_gemini_oauth_login"),
  installGeminiCli: (useBun: boolean): Promise<boolean> =>
    invoke("install_gemini_cli", { useBun }),
  getCodexOauthQuota: (accountId: string | null): Promise<SubscriptionQuota> =>
    invoke("get_codex_oauth_quota", { accountId }),
  getCodingPlanQuota: (
    baseUrl: string,
    apiKey: string,
  ): Promise<SubscriptionQuota> =>
    invoke("get_coding_plan_quota", { baseUrl, apiKey }),
  getBalance: (
    baseUrl: string,
    apiKey: string,
  ): Promise<import("@/types").UsageResult> =>
    invoke("get_balance", { baseUrl, apiKey }),
};
