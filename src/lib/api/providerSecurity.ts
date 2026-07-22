import { invoke } from "@tauri-apps/api/core";
import type { AppId } from "./types";
import type {
  ExplicitCredentialImport,
  MutationOutcome,
  ProviderSecurityStatus,
  RecoveryMode,
  RecoveryResult,
} from "@/types/providerSecurity";

export const providerSecurityApi = {
  async status(
    appId: AppId,
    providerId: string,
  ): Promise<ProviderSecurityStatus> {
    return await invoke("get_provider_security_status", {
      app: appId,
      id: providerId,
    });
  },

  async importLiveCredentials(
    args: ExplicitCredentialImport,
  ): Promise<MutationOutcome> {
    return await invoke("import_live_provider_credentials", {
      app: args.appId,
      id: args.providerId,
      expectedRevision: args.expectedRevision,
      fields: args.fields,
    });
  },

  async recover(appId: AppId, mode: RecoveryMode): Promise<RecoveryResult> {
    return await invoke("recover_app_configuration", {
      app: appId,
      mode,
    });
  },
};
