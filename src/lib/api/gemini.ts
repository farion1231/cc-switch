import { invoke } from "@tauri-apps/api/core";
import type {
  GeminiAccount,
  GeminiLoginSession,
  GeminiLoginStatusView,
  GeminiPoolStatus,
  GeminiUsageView,
  RefreshResult,
} from "@/types";

const normalizeLoginSession = (
  session: GeminiLoginSession,
): GeminiLoginSession => {
  const authUrl = session.authUrl ?? session.verificationUrl;
  const expectedFilesDir =
    session.expectedFilesDir ?? session.credentialDir;
  return {
    ...session,
    authUrl,
    expectedFilesDir,
    verificationUrl: session.verificationUrl ?? authUrl,
    credentialDir: session.credentialDir ?? expectedFilesDir,
  };
};

const normalizeLoginStatus = (
  status: GeminiLoginStatusView,
): GeminiLoginStatusView => {
  const authUrl = status.authUrl ?? status.verificationUrl;
  const expectedFilesDir = status.expectedFilesDir ?? status.credentialDir;
  return {
    ...status,
    authUrl,
    expectedFilesDir,
    verificationUrl: status.verificationUrl ?? authUrl,
    credentialDir: status.credentialDir ?? expectedFilesDir,
  };
};

export const geminiApi = {
  async startCliLogin(providerId: string): Promise<GeminiLoginSession> {
    const session = await invoke<GeminiLoginSession>("gemini_start_cli_login", {
      providerId,
    });
    return normalizeLoginSession(session);
  },

  async getCliLoginStatus(sessionId: string): Promise<GeminiLoginStatusView> {
    const status = await invoke<GeminiLoginStatusView>(
      "gemini_get_cli_login_status",
      { sessionId },
    );
    return normalizeLoginStatus(status);
  },

  async cancelCliLogin(sessionId: string): Promise<void> {
    await invoke("gemini_cancel_cli_login", { sessionId });
  },

  async finalizeCliLogin(sessionId: string): Promise<GeminiAccount> {
    return await invoke("gemini_finalize_cli_login", { sessionId });
  },

  async listAccounts(): Promise<GeminiAccount[]> {
    return await invoke("gemini_list_accounts");
  },

  async getUsageState(providerId: string): Promise<GeminiUsageView> {
    return await invoke("gemini_get_usage_state", { providerId });
  },

  async refreshUsageNow(providerId?: string): Promise<RefreshResult> {
    return await invoke("gemini_refresh_usage_now", { providerId });
  },

  async poolStatus(): Promise<GeminiPoolStatus> {
    return await invoke("gemini_pool_status");
  },
};
