import { invoke } from "@tauri-apps/api/core";
import type {
  CodexModelRoutingConfig,
  ModelRoutingSaveResult,
} from "@/types/codexModelRouting";

export const codexModelRoutingApi = {
  get: () => invoke<CodexModelRoutingConfig>("get_codex_model_routing"),
  save: (config: CodexModelRoutingConfig) =>
    invoke<ModelRoutingSaveResult>("save_codex_model_routing", { config }),
  setEnabled: (enabled: boolean) =>
    invoke<void>("set_codex_model_routing_enabled", { enabled }),
};
