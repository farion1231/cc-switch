import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { isCliWebUi } from "@/lib/platform";
import type { RuntimeInfo } from "./runtime";

type TauriInvokeArgs = Parameters<typeof tauriInvoke>[1];
type TauriInvokeOptions = Parameters<typeof tauriInvoke>[2];

export interface RemoteBackendConfig {
  url: string;
  token?: string;
}

export interface ClientBackendConnectionSettings {
  mode: "local" | "remote";
  url?: string | null;
  token?: string | null;
}

declare global {
  interface Window {
    __CC_SWITCH_BACKEND_URL__?: string;
    __CC_SWITCH_BACKEND_TOKEN__?: string;
  }
}

const BACKEND_URL_KEY = "cc-switch-backend-url";
const BACKEND_TOKEN_KEY = "cc-switch-backend-token";

const trimTrailingSlash = (value: string): string =>
  value.trim().replace(/\/+$/, "");

const normalizeRemoteBackendConfig = (
  value: RemoteBackendConfig,
): RemoteBackendConfig => ({
  url: trimTrailingSlash(value.url),
  token: value.token?.trim() || undefined,
});

const readStorageValue = (
  storage: Storage | undefined,
  key: string,
): string | undefined => {
  try {
    return storage?.getItem(key)?.trim() || undefined;
  } catch {
    return undefined;
  }
};

const writeSessionValue = (key: string, value: string): void => {
  try {
    window.sessionStorage.setItem(key, value);
  } catch {
    // Session storage can be unavailable in restricted browser contexts.
  }
};

const writeLocalValue = (key: string, value: string): void => {
  try {
    window.localStorage.setItem(key, value);
  } catch {
    // Local storage can be unavailable in restricted browser contexts.
  }
};

const removeStorageValue = (key: string): void => {
  try {
    window.sessionStorage.removeItem(key);
  } catch {
    // Ignore unavailable session storage.
  }
  try {
    window.localStorage.removeItem(key);
  } catch {
    // Ignore unavailable local storage.
  }
};

const removeSensitiveQueryParams = (names: string[]): void => {
  if (typeof window === "undefined") return;

  const params = new URLSearchParams(window.location.search);
  let changed = false;
  for (const name of names) {
    if (params.has(name)) {
      params.delete(name);
      changed = true;
    }
  }
  if (!changed) return;

  const search = params.toString();
  const nextUrl = `${window.location.pathname}${search ? `?${search}` : ""}${window.location.hash}`;
  window.history.replaceState(window.history.state, "", nextUrl);
};

export const getRemoteBackendConfig = (): RemoteBackendConfig | null => {
  if (typeof window === "undefined") return null;

  const params = new URLSearchParams(window.location.search);
  const paramUrl = params.get("backendUrl") || params.get("backend");
  const paramToken = params.get("backendToken");

  if (paramUrl?.trim()) {
    writeSessionValue(BACKEND_URL_KEY, paramUrl.trim());
  }
  if (paramToken?.trim()) {
    writeSessionValue(BACKEND_TOKEN_KEY, paramToken.trim());
    removeSensitiveQueryParams(["backendToken"]);
  }

  const rawUrl =
    window.__CC_SWITCH_BACKEND_URL__?.trim() ||
    paramUrl?.trim() ||
    readStorageValue(window.sessionStorage, BACKEND_URL_KEY) ||
    readStorageValue(window.localStorage, BACKEND_URL_KEY);

  if (!rawUrl) return null;

  const token =
    window.__CC_SWITCH_BACKEND_TOKEN__?.trim() ||
    paramToken?.trim() ||
    readStorageValue(window.sessionStorage, BACKEND_TOKEN_KEY) ||
    readStorageValue(window.localStorage, BACKEND_TOKEN_KEY);

  return {
    url: trimTrailingSlash(rawUrl),
    token,
  };
};

export const hasRemoteBackendOverride = (): boolean =>
  getRemoteBackendConfig() !== null;

export const setRemoteBackendOverride = (config: RemoteBackendConfig): void => {
  if (typeof window === "undefined") return;
  const normalized = normalizeRemoteBackendConfig(config);
  writeLocalValue(BACKEND_URL_KEY, normalized.url);
  if (normalized.token) {
    writeLocalValue(BACKEND_TOKEN_KEY, normalized.token);
  } else {
    removeStorageValue(BACKEND_TOKEN_KEY);
  }
};

export const clearRemoteBackendOverride = (): void => {
  if (typeof window === "undefined") return;
  removeStorageValue(BACKEND_URL_KEY);
  removeStorageValue(BACKEND_TOKEN_KEY);
};

let clientBackendConfigPromise: Promise<RemoteBackendConfig | null> | null =
  null;

const getClientBackendConnectionConfig =
  async (): Promise<RemoteBackendConfig | null> => {
    if (isCliWebUi()) return null;
    try {
      const connection = await invokeLocal<ClientBackendConnectionSettings>(
        "get_client_backend_connection",
      );
      if (connection.mode !== "remote" || !connection.url?.trim()) {
        return null;
      }
      return normalizeRemoteBackendConfig({
        url: connection.url,
        token: connection.token?.trim() || undefined,
      });
    } catch {
      return null;
    }
  };

export const getEffectiveRemoteBackendConfig =
  async (): Promise<RemoteBackendConfig | null> => {
    const override = getRemoteBackendConfig();
    if (override) return normalizeRemoteBackendConfig(override);

    clientBackendConfigPromise ??= getClientBackendConnectionConfig();
    return await clientBackendConfigPromise;
  };

export const clearClientBackendConfigCache = (): void => {
  clientBackendConfigPromise = null;
};

export async function getClientBackendConnection(): Promise<ClientBackendConnectionSettings> {
  return await invokeLocal<ClientBackendConnectionSettings>(
    "get_client_backend_connection",
  );
}

export async function saveClientBackendConnection(
  connection: ClientBackendConnectionSettings,
): Promise<boolean> {
  clearClientBackendConfigCache();
  return await invokeLocal("save_client_backend_connection", { connection });
}

export async function invokeLocal<T>(
  cmd: string,
  args?: TauriInvokeArgs,
  options?: TauriInvokeOptions,
): Promise<T> {
  return await tauriInvoke<T>(cmd, args, options);
}

async function invokeRemote<T>(
  remote: RemoteBackendConfig,
  cmd: string,
  args?: TauriInvokeArgs,
): Promise<T> {
  const normalized = normalizeRemoteBackendConfig(remote);

  const response = await fetch(`${normalized.url}/__cc_switch_webui__/invoke`, {
    method: "POST",
    headers: {
      "content-type": "application/json",
      ...(normalized.token
        ? { "x-cc-switch-webui-token": normalized.token }
        : {}),
    },
    body: JSON.stringify({ cmd, args: args ?? {} }),
  });

  const text = await response.text();
  const payload = text ? JSON.parse(text) : null;
  if (!response.ok) {
    throw payload?.error ?? payload ?? new Error(`Backend request failed`);
  }
  return payload as T;
}

export async function testRemoteBackendConnection(
  config: RemoteBackendConfig,
): Promise<RuntimeInfo> {
  return await invokeRemote<RuntimeInfo>(config, "get_runtime_info");
}

export async function invoke<T>(
  cmd: string,
  args?: TauriInvokeArgs,
  options?: TauriInvokeOptions,
): Promise<T> {
  const remote = await getEffectiveRemoteBackendConfig();
  if (!remote) {
    return await invokeLocal<T>(cmd, args, options);
  }

  return await invokeRemote<T>(remote, cmd, args);
}
