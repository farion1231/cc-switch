import { get, put } from "../web-client";
import type {
  OpenClawAgentsDefaults,
  OpenClawDefaultModel,
  OpenClawEnvConfig,
  OpenClawHealthWarning,
  OpenClawModelCatalogEntry,
  OpenClawToolsConfig,
  OpenClawWriteOutcome,
} from "@/types";

// Web (HTTP) implementation of the OpenClaw API. Mirrors the Tauri surface in
// src/lib/api/openclaw.ts so the runtime selector can swap them transparently.
export const openclawApi = {
  async getDefaultModel(): Promise<OpenClawDefaultModel | null> {
    return get("/openclaw/default-model");
  },

  async setDefaultModel(
    model: OpenClawDefaultModel,
  ): Promise<OpenClawWriteOutcome> {
    return put("/openclaw/default-model", model);
  },

  async getModelCatalog(): Promise<Record<
    string,
    OpenClawModelCatalogEntry
  > | null> {
    return get("/openclaw/model-catalog");
  },

  async setModelCatalog(
    catalog: Record<string, OpenClawModelCatalogEntry>,
  ): Promise<OpenClawWriteOutcome> {
    return put("/openclaw/model-catalog", catalog);
  },

  async getAgentsDefaults(): Promise<OpenClawAgentsDefaults | null> {
    return get("/openclaw/agents-defaults");
  },

  async setAgentsDefaults(
    defaults: OpenClawAgentsDefaults,
  ): Promise<OpenClawWriteOutcome> {
    return put("/openclaw/agents-defaults", defaults);
  },

  async getEnv(): Promise<OpenClawEnvConfig> {
    return get("/openclaw/env");
  },

  async setEnv(env: OpenClawEnvConfig): Promise<OpenClawWriteOutcome> {
    return put("/openclaw/env", env);
  },

  async getTools(): Promise<OpenClawToolsConfig> {
    return get("/openclaw/tools");
  },

  async setTools(tools: OpenClawToolsConfig): Promise<OpenClawWriteOutcome> {
    return put("/openclaw/tools", tools);
  },

  async scanHealth(): Promise<OpenClawHealthWarning[]> {
    return get("/openclaw/health");
  },

  async getLiveProvider(
    providerId: string,
  ): Promise<Record<string, unknown> | null> {
    return get(`/openclaw/live-provider/${encodeURIComponent(providerId)}`);
  },
};
