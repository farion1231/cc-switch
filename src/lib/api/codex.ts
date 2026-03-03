import { invoke } from "@tauri-apps/api/core";
import type {
  CodexAccount,
  CodexUsageView,
  ImportResult,
  LoginSession,
  RefreshResult,
} from "@/types";

export const codexApi = {
  async listAccounts(): Promise<CodexAccount[]> {
    return await invoke("codex_list_accounts");
  },

  async startLogin(providerId: string): Promise<LoginSession> {
    return await invoke("codex_start_login", { providerId });
  },

  async completeLogin(
    sessionId: string,
    callbackPayload: string,
  ): Promise<CodexAccount> {
    return await invoke("codex_complete_login", { sessionId, callbackPayload });
  },

  async importFromSwitcherOnce(): Promise<ImportResult> {
    return await invoke("codex_import_from_switcher_once");
  },

  async getUsageState(providerId: string): Promise<CodexUsageView> {
    return await invoke("codex_get_usage_state", { providerId });
  },

  async refreshUsageNow(providerId?: string): Promise<RefreshResult> {
    return await invoke("codex_refresh_usage_now", { providerId });
  },

  async bindProviderAuth(providerId: string): Promise<CodexAccount> {
    return await invoke("codex_bind_provider_auth", { providerId });
  },
};
