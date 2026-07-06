import { invoke } from "@tauri-apps/api/core";
import type {
  PiConnectivityResult,
  PiProviderApplyResult,
  PiProviderDraft,
  PiProviderPatchPreview,
  PiProvidersMap,
} from "@/types/pi";

export const piApi = {
  async listProviders(): Promise<PiProvidersMap> {
    return await invoke("list_pi_providers");
  },

  async previewProviderPatch(
    draft: PiProviderDraft,
  ): Promise<PiProviderPatchPreview> {
    return await invoke("preview_pi_provider_patch", { draft });
  },

  async applyProviderPatch(
    draft: PiProviderDraft,
    expectedFileHash: string,
  ): Promise<PiProviderApplyResult> {
    return await invoke("apply_pi_provider_patch", {
      draft,
      expectedFileHash,
    });
  },

  async deleteProvider(
    providerId: string,
    expectedFileHash: string,
  ): Promise<PiProviderApplyResult> {
    return await invoke("delete_pi_provider", {
      providerId,
      expectedFileHash,
    });
  },

  async testConnectivity(providerId: string): Promise<PiConnectivityResult> {
    return await invoke("test_pi_connectivity", { providerId });
  },
};
