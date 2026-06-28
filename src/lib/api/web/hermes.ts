import { get, post, put } from "../web-client";
import type {
  HermesMemoryKind,
  HermesMemoryLimits,
  HermesModelConfig,
} from "@/types";

// Web (HTTP) implementation of the Hermes API. Mirrors the Tauri surface in
// src/lib/api/hermes.ts so the runtime selector in src/lib/api/index.ts can
// swap them transparently.
export const hermesApi = {
  async getModelConfig(): Promise<HermesModelConfig | null> {
    return get("/hermes/model-config");
  },

  /**
   * Resolve the live Hermes Web UI URL server-side and open it in a new browser
   * tab. The backend probes the local Hermes server and returns the URL, or an
   * envelope error of `hermes_web_offline` which surfaces here as a thrown
   * Error with that exact message (matched by useOpenHermesWebUI).
   */
  async openWebUI(path?: string): Promise<void> {
    const url = await post<string>("/hermes/open-web-ui", {
      path: path ?? null,
    });
    if (typeof window !== "undefined" && url) {
      window.open(url, "_blank", "noopener,noreferrer");
    }
  },

  async launchDashboard(): Promise<void> {
    await post("/hermes/launch-dashboard", {});
  },

  async getMemory(kind: HermesMemoryKind): Promise<string> {
    return get(`/hermes/memory/${encodeURIComponent(kind)}`);
  },

  async setMemory(kind: HermesMemoryKind, content: string): Promise<void> {
    await put(`/hermes/memory/${encodeURIComponent(kind)}`, { content });
  },

  async getMemoryLimits(): Promise<HermesMemoryLimits> {
    return get("/hermes/memory-limits");
  },

  async setMemoryEnabled(
    kind: HermesMemoryKind,
    enabled: boolean,
  ): Promise<void> {
    await post(`/hermes/memory/${encodeURIComponent(kind)}/enabled`, {
      enabled,
    });
  },
};
