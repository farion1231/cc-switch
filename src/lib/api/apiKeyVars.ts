import { invoke } from "@tauri-apps/api/core";

/** 变量刷新运行状态（时间均为 Unix 毫秒） */
export interface ApiKeyVarState {
  expiresAt?: number;
  refreshAfter?: number;
  lastSuccessAt?: number;
  lastAttemptAt?: number;
  lastError?: string;
  lastMessage?: string;
  resolvedScript?: string;
  consecutiveFailures: number;
  needsAttention: boolean;
}

export interface ApiKeyVarReference {
  app: string;
  providerId: string;
  providerName: string;
  isCurrent: boolean;
}

/** 后端返回的变量信息：只含掩码值，绝不含明文 */
export interface ApiKeyVar {
  name: string;
  enabled: boolean;
  scriptDir: string;
  timeoutSecs?: number;
  leadTimeSecs?: number;
  maskedValue?: string;
  hasValue: boolean;
  state: ApiKeyVarState;
  references: ApiKeyVarReference[];
}

export interface ApiKeyVarInput {
  name: string;
  scriptDir: string;
  enabled: boolean;
  timeoutSecs?: number;
  leadTimeSecs?: number;
}

export interface ResolvedScript {
  dir: string;
  path: string;
  file: string;
  ignoredIndexJs: boolean;
}

export interface ApiKeyVarTemplateFile {
  path: string;
  content: string;
}

export interface ApiKeyVarTestOutcome {
  apiKeyMasked: string;
  expiresAt?: number;
  refreshAfter?: number;
  message?: string;
  file: string;
}

export interface ApiKeyVarRefreshOutcome {
  success: boolean;
  name: string;
  maskedValue?: string;
  expiresAt?: number;
  message?: string;
  error?: string;
  resolvedScript?: string;
  needsAttention: boolean;
  valueExpired: boolean;
  updatedProviders: ApiKeyVarReference[];
  liveErrors: string[];
}

export type ApiKeyRefStatus =
  | "ok"
  | "unknown"
  | "disabled"
  | "noValue"
  | "malformed";

export const API_KEY_VAR_REFRESHED_EVENT = "api-key-var-refreshed";
export const API_KEY_VAR_REFRESH_FAILED_EVENT = "api-key-var-refresh-failed";
export const API_KEY_VARS_QUERY_KEY = ["apiKeyVars"] as const;

/** 变量名规则：与后端 `resolve::is_valid_name` 一致 */
export const API_KEY_VAR_NAME_RE = /^[A-Z_][A-Z0-9_]*$/;
export const API_KEY_VAR_NAME_MAX = 64;
export const API_KEY_VAR_RESERVED_PREFIX = "CC_SWITCH_";

export function isValidApiKeyVarName(name: string): boolean {
  return (
    name.length > 0 &&
    name.length <= API_KEY_VAR_NAME_MAX &&
    API_KEY_VAR_NAME_RE.test(name) &&
    !name.startsWith(API_KEY_VAR_RESERVED_PREFIX)
  );
}

/** 整字段引用：trim 后恰好是 `{{NAME}}` 时返回 NAME */
export function parseApiKeyRef(value: string): string | null {
  const m = /^\{\{([A-Z_][A-Z0-9_]*)\}\}$/.exec(value.trim());
  return m && m[1].length <= API_KEY_VAR_NAME_MAX ? m[1] : null;
}

/** 含 `{{` / `}}` 但不是合法的整字段引用 */
export function isMalformedApiKeyRef(value: string): boolean {
  return (
    (value.includes("{{") || value.includes("}}")) &&
    parseApiKeyRef(value) === null
  );
}

/** 按变量列表计算一个 Key 输入值的引用状态；不是引用时返回 null */
export function apiKeyRefStatus(
  value: string,
  vars: ApiKeyVar[] | undefined,
): { name: string; status: ApiKeyRefStatus; variable?: ApiKeyVar } | null {
  if (isMalformedApiKeyRef(value)) {
    return { name: value.trim(), status: "malformed" };
  }
  const name = parseApiKeyRef(value);
  if (!name) return null;
  const variable = vars?.find((v) => v.name === name);
  if (!variable) return { name, status: "unknown" };
  if (!variable.enabled) return { name, status: "disabled", variable };
  if (!variable.hasValue) return { name, status: "noValue", variable };
  return { name, status: "ok", variable };
}

export const apiKeyVarsApi = {
  async getTemplate(): Promise<ApiKeyVarTemplateFile[]> {
    return await invoke("get_api_key_var_template");
  },
  async downloadTemplate(): Promise<string | null> {
    return await invoke("download_api_key_var_template");
  },
  async list(): Promise<ApiKeyVar[]> {
    return await invoke("list_api_key_vars");
  },
  async upsert(
    input: ApiKeyVarInput,
    originalName?: string,
  ): Promise<ApiKeyVar> {
    return await invoke("upsert_api_key_var", {
      input,
      originalName: originalName ?? null,
    });
  },
  async remove(name: string): Promise<void> {
    await invoke("delete_api_key_var", { name });
  },
  async resolveScript(dir: string): Promise<ResolvedScript> {
    return await invoke("resolve_api_key_var_script", { dir });
  },
  async test(input: ApiKeyVarInput): Promise<ApiKeyVarTestOutcome> {
    return await invoke("test_api_key_var", { input });
  },
  async refresh(name: string): Promise<ApiKeyVarRefreshOutcome> {
    return await invoke("refresh_api_key_var", { name });
  },
};
