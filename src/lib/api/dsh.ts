import { invoke } from "@tauri-apps/api/core";

/**
 * DSH's two provider registries.  These values are deliberately separate from
 * cc-switch's {@link AppId}: a DSH route is a live settings-file entry, not a
 * database provider row.
 */
export type DshProviderKind = "native" | "custom";

/** A model descriptor exposed by DSH's local catalog. */
export interface DshModel {
  id: string;
  name?: string;
  description?: string;
  contextWindow?: number;
  maxTokens?: number;
  /** Forward-compatible fields from newer DSH model descriptors. */
  [key: string]: unknown;
}

/** Credential metadata safe to return to a browser surface. */
export interface DshCredentialInfo {
  ref: string;
  configured: boolean;
  source?: string;
  writable: boolean;
}

/** A warning about a configuration feature this page intentionally leaves untouched. */
export interface DshUnsupportedWarning {
  code: string;
  message?: string;
  namespace?: string;
  path?: string[];
}

/** One native or custom route read from DSH settings. */
export interface DshProvider {
  route: string;
  kind: DshProviderKind;
  displayName: string;
  api?: string;
  baseURL?: string;
  models: DshModel[];
  /** Reference only; this field is never a secret value. */
  apiKeyEnv?: string;
  credential?: DshCredentialInfo;
  customized: boolean;
  /** Revision of the section used to render this row, when supplied by DSH. */
  revision?: string;
  /** Number of model entries DSH resolved, for older backends that omit models. */
  modelCount?: number;
}

/** The model used for newly composed DSH agents. */
export interface DshDefaultModel {
  provider: string;
  model: string;
  reasoningEffort?: string;
}

/** A complete, redacted live DSH view. */
export interface DshSnapshot {
  home: string;
  settingsPath: string;
  credentialsPath: string;
  settingsRevision: string;
  credentialsRevision?: string;
  readOnly: boolean;
  unsupported?: DshUnsupportedWarning[];
  providers: DshProvider[];
  defaultModel: DshDefaultModel | null;
  protocols: string[];
  refreshedAt?: number;
}

/** Native route fields accepted by the dedicated DSH command. */
export interface DshNativeInput {
  baseURL?: string;
  models?: DshModel[];
  apiKeyEnv?: string;
  /** Optional expected section revision for stale-editor protection. */
  expectedRevision?: string;
}

/** A custom route profile accepted by the dedicated DSH command. */
export interface DshCustomInput {
  route: string;
  displayName?: string;
  api: string;
  baseURL: string;
  models: DshModel[];
  apiKeyEnv?: string;
  /** Fields not rendered by this page are kept by the backend on update. */
  expectedRevision?: string;
}

/** One-way credential write.  Never use this type as a response type. */
export interface DshCredentialWrite {
  ref: string;
  value: string;
  /** Expected credentials document revision for stale-editor protection. */
  expectedRevision?: string;
}

/** Input for model discovery; `apiKey` is write-only and never returned. */
export interface DshModelDiscoveryInput {
  baseURL: string;
  api: string;
  apiKey?: string;
  /** Existing route credential reference used only for a one-shot probe. */
  credentialRef?: string;
}

/** Result from a discovery call. */
export interface DshModelDiscoveryResult {
  models: DshModel[];
}

/** Structured error emitted by the Rust DSH command family. */
export interface DshErrorPayload {
  code?: string;
  message?: string;
  detail?: string;
  namespace?: string;
  path?: string[];
}

/** Commands may return the refreshed snapshot directly or under `snapshot`. */
export type DshMutationResponse =
  | DshSnapshot
  | { snapshot?: DshSnapshot; ok?: boolean };

/** A conflict/error code that should preserve the user's draft for retry. */
export const DSH_CONFLICT_CODES = new Set([
  "settings-conflict",
  "credentials-conflict",
  "stale-revision",
  "dsh-stale-revision",
]);

/**
 * Extract a non-secret DSH error payload from a Tauri rejection.
 * @param error - unknown value rejected by `invoke`.
 * @returns a safe error payload; secret-bearing fields are intentionally ignored.
 */
export function readDshError(error: unknown): DshErrorPayload {
  if (typeof error === "string") {
    try {
      return readDshError(JSON.parse(error) as unknown);
    } catch {
      return { message: error };
    }
  }
  if (!(error && typeof error === "object")) return {};
  const value = error as Record<string, unknown>;
  const nested = value.payload;
  const source =
    nested && typeof nested === "object"
      ? (nested as Record<string, unknown>)
      : value;
  const code = typeof source.code === "string" ? source.code : undefined;
  const message =
    typeof source.message === "string" ? source.message : undefined;
  const detail = typeof source.detail === "string" ? source.detail : undefined;
  const namespace =
    typeof source.namespace === "string" ? source.namespace : undefined;
  const path = Array.isArray(source.path)
    ? source.path.filter((part): part is string => typeof part === "string")
    : undefined;
  return { code, message, detail, namespace, path };
}

/**
 * Decide whether an error is a stale-file conflict without exposing its text.
 * @param error - unknown Tauri rejection.
 * @returns true when the caller should refetch and keep the draft open.
 */
export function isDshConflictError(error: unknown): boolean {
  const payload = readDshError(error);
  if (payload.code && DSH_CONFLICT_CODES.has(payload.code)) return true;
  const text = `${payload.message ?? ""} ${payload.detail ?? ""}`.toLowerCase();
  return (
    text.includes("revision") &&
    (text.includes("stale") || text.includes("conflict"))
  );
}

/** Return a bounded user-facing error string, omitting untrusted object dumps. */
export function dshErrorMessage(error: unknown, fallback: string): string {
  const payload = readDshError(error);
  const text = payload.message ?? payload.detail;
  if (!text || text.length > 500) return fallback;
  return text;
}

function unwrapSnapshot(
  response: DshMutationResponse,
): DshSnapshot | undefined {
  if ("home" in response && "providers" in response) return response;
  return response.snapshot;
}

/** Dedicated live API for DeepSeek Harness files and provider routes. */
export const dshApi = {
  /** Read and reconcile the current DSH settings/credentials documents. */
  async getSnapshot(): Promise<DshSnapshot> {
    return await invoke<DshSnapshot>("dsh_get_snapshot");
  },

  /** Explicitly re-read external edits; this never writes a cc-switch record. */
  async refresh(): Promise<DshSnapshot> {
    return await invoke<DshSnapshot>("dsh_refresh");
  },

  /** Update only native DeepSeek user-owned fields. */
  async upsertNative(input: DshNativeInput): Promise<DshSnapshot | undefined> {
    const result = await invoke<DshMutationResponse>("dsh_upsert_native", {
      baseUrl: input.baseURL,
      models: input.models,
      apiKeyEnv: input.apiKeyEnv,
      expectedRevision: input.expectedRevision,
    });
    return unwrapSnapshot(result);
  },

  /** Unset only native route user overrides; the route itself remains present. */
  async resetNative(
    expectedRevision?: string,
  ): Promise<DshSnapshot | undefined> {
    const result = await invoke<DshMutationResponse>("dsh_reset_native", {
      expectedRevision,
    });
    return unwrapSnapshot(result);
  },

  /** Create a custom OpenAI/Anthropic-compatible route. */
  async createCustom(input: DshCustomInput): Promise<DshSnapshot | undefined> {
    const result = await invoke<DshMutationResponse>("dsh_create_custom", {
      route: input.route,
      displayName: input.displayName,
      api: input.api,
      baseUrl: input.baseURL,
      models: input.models,
      apiKeyEnv: input.apiKeyEnv,
      expectedRevision: input.expectedRevision,
    });
    return unwrapSnapshot(result);
  },

  /** Update a custom route while preserving backend-owned unknown fields. */
  async updateCustom(input: DshCustomInput): Promise<DshSnapshot | undefined> {
    const result = await invoke<DshMutationResponse>("dsh_update_custom", {
      route: input.route,
      displayName: input.displayName,
      api: input.api,
      baseUrl: input.baseURL,
      models: input.models,
      apiKeyEnv: input.apiKeyEnv,
      expectedRevision: input.expectedRevision,
    });
    return unwrapSnapshot(result);
  },

  /** Remove one custom route; native route removal is rejected by the backend. */
  async removeCustom(
    route: string,
    expectedRevision?: string,
  ): Promise<DshSnapshot | undefined> {
    const result = await invoke<DshMutationResponse>("dsh_remove_custom", {
      route,
      expectedRevision,
    });
    return unwrapSnapshot(result);
  },

  /** Save the complete default provider/model selection. */
  async setDefaultModel(
    selection: DshDefaultModel,
    expectedRevision?: string,
  ): Promise<DshSnapshot | undefined> {
    const result = await invoke<DshMutationResponse>("dsh_set_default_model", {
      selection,
      expectedRevision,
    });
    return unwrapSnapshot(result);
  },

  /** Store one API key in the DSH credentials provider; response has no value. */
  async setCredential(input: DshCredentialWrite): Promise<void> {
    await invoke<void>("dsh_set_credential", {
      reference: input.ref,
      value: input.value,
      expectedRevision: input.expectedRevision,
    });
  },

  /** Remove one credential reference after an explicit user confirmation. */
  async unsetCredential(ref: string, expectedRevision?: string): Promise<void> {
    await invoke<void>("dsh_unset_credential", {
      reference: ref,
      expectedRevision,
    });
  },

  /** Probe a compatible endpoint; `apiKey` is never returned or persisted. */
  async discoverModels(
    input: DshModelDiscoveryInput,
  ): Promise<DshModelDiscoveryResult> {
    return await invoke<DshModelDiscoveryResult>("dsh_discover_models", {
      baseUrl: input.baseURL,
      api: input.api,
      apiKey: input.apiKey,
      credentialRef: input.credentialRef,
    });
  },

  /** Ask the OS to open the resolved DSH home directory. */
  async openHome(): Promise<void> {
    await invoke<void>("dsh_open_home");
  },
};

export type { DshMutationResponse as DshCommandResult };
