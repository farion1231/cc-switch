type EventHandler<T> = (event: { payload: T }) => void;
type UnlistenFn = () => void;

declare global {
  interface Window {
    __TAURI__?: unknown;
    __TAURI_INTERNALS__?: unknown;
  }
}

export function isTauriRuntime(): boolean {
  return Boolean(
    typeof window !== "undefined" &&
      (window.__TAURI_INTERNALS__ || window.__TAURI__),
  );
}

function webUiBaseUrl(): string {
  const configured = import.meta.env.VITE_CC_SWITCH_WEBUI_API_BASE as
    | string
    | undefined;
  if (configured) return configured.replace(/\/$/, "");

  if (typeof window === "undefined") return "http://127.0.0.1:15722";
  const { protocol, hostname, port } = window.location;
  return `${protocol}//${hostname}${port ? `:${port}` : ""}`;
}

async function request<T>(path: string, options: RequestInit = {}): Promise<T> {
  const headers = new Headers(options.headers);
  if (options.body && !headers.has("Content-Type")) {
    headers.set("Content-Type", "application/json");
  }

  const response = await fetch(`${webUiBaseUrl()}${path}`, {
    ...options,
    headers,
    credentials: "include",
  });

  const text = await response.text();
  const data = text ? JSON.parse(text) : null;
  if (!response.ok) {
    const error = new Error(
      (data && typeof data === "object" && "error" in data
        ? String((data as { error?: unknown }).error)
        : undefined) ||
        response.statusText ||
        "WebUI request failed",
    ) as Error & { requiresLogin?: boolean };
    if (
      data &&
      typeof data === "object" &&
      "requiresLogin" in data &&
      (data as { requiresLogin?: unknown }).requiresLogin === true
    ) {
      error.requiresLogin = true;
    }
    throw error;
  }
  return data as T;
}

const post = <T>(path: string, body?: unknown) =>
  request<T>(path, {
    method: "POST",
    body: body === undefined ? undefined : JSON.stringify(body),
  });

export interface WebUiAuthStatus {
  authRequired: boolean;
  authenticated: boolean;
}

export async function getWebUiAuthStatus(): Promise<WebUiAuthStatus> {
  return request<WebUiAuthStatus>("/api/webui/auth-status");
}

export async function loginWebUi(password: string): Promise<void> {
  await post<void>("/api/webui/login", { password });
}

export async function invokeCommand<T = unknown>(
  command: string,
  args: Record<string, unknown> = {},
): Promise<T> {
  if (isTauriRuntime()) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<T>(command, args);
  }

  switch (command) {
    case "get_settings":
      return request<T>("/api/settings");
    case "save_settings":
      return post<T>("/api/settings", args.settings);
    case "is_portable_mode":
      return false as T;
    case "open_external":
      if (typeof args.url === "string")
        window.open(args.url, "_blank", "noopener");
      return true as T;

    case "get_providers":
      return request<T>(
        `/api/providers?app=${encodeURIComponent(String(args.app))}`,
      );
    case "get_current_provider":
      return request<T>(
        `/api/providers/current?app=${encodeURIComponent(String(args.app))}`,
      );
    case "add_provider":
      return post<T>("/api/providers/add", args);
    case "update_provider":
      return post<T>("/api/providers/update", args);
    case "delete_provider":
      return post<T>("/api/providers/delete", args);
    case "remove_provider_from_live_config":
      return post<T>("/api/providers/remove-live", args);
    case "switch_provider":
      return post<T>("/api/providers/switch", args);
    case "import_default_config":
      return post<T>("/api/providers/import-default", args);
    case "import_claude_desktop_providers_from_claude":
      return post<T>("/api/claude-desktop/import-from-claude");
    case "update_providers_sort_order":
      return post<T>("/api/providers/sort", args);
    case "get_claude_desktop_status":
      return request<T>("/api/claude-desktop/status");
    case "get_claude_desktop_default_routes":
      return request<T>("/api/claude-desktop/default-routes");
    case "update_tray_menu":
      return post<T>("/api/tray/update");
    case "get_opencode_live_provider_ids":
      return request<T>("/api/opencode/live-provider-ids");
    case "import_opencode_providers_from_live":
      return post<T>("/api/opencode/import-live");
    case "get_openclaw_live_provider_ids":
      return request<T>("/api/openclaw/live-provider-ids");
    case "import_openclaw_providers_from_live":
      return post<T>("/api/openclaw/import-live");
    case "get_hermes_live_provider_ids":
      return request<T>("/api/hermes/live-provider-ids");
    case "import_hermes_providers_from_live":
      return post<T>("/api/hermes/import-live");

    case "start_proxy_server":
      return post<T>("/api/proxy/start");
    case "stop_proxy_server":
      return post<T>("/api/proxy/stop");
    case "stop_proxy_with_restore":
      return post<T>("/api/proxy/stop-with-restore");
    case "get_proxy_status":
      return request<T>("/api/proxy/status");
    case "is_proxy_running":
      return request<T>("/api/proxy/running");
    case "is_live_takeover_active":
      return request<T>("/api/proxy/live-takeover-active");
    case "get_proxy_takeover_status":
      return request<T>("/api/proxy/takeover-status");
    case "set_proxy_takeover_for_app":
      return post<T>("/api/proxy/takeover", args);
    case "switch_proxy_provider":
      return post<T>("/api/proxy/switch-provider", args);
    case "get_proxy_config":
      return request<T>("/api/proxy/config");
    case "update_proxy_config":
      return post<T>("/api/proxy/config", args);
    case "get_global_proxy_config":
      return request<T>("/api/proxy/global-config");
    case "update_global_proxy_config":
      return post<T>("/api/proxy/global-config", args);
    case "get_global_proxy_url":
      return request<T>("/api/proxy/global-url");
    case "set_global_proxy_url":
      return post<T>("/api/proxy/global-url", args);
    case "test_proxy_url":
      return post<T>("/api/proxy/test-url", args);
    case "get_upstream_proxy_status":
      return request<T>("/api/proxy/upstream-status");
    case "get_proxy_config_for_app":
      return post<T>("/api/proxy/app-config", args);
    case "update_proxy_config_for_app":
      return post<T>("/api/proxy/app-config/update", args);
    case "get_default_cost_multiplier":
      return post<T>("/api/proxy/default-cost-multiplier", args);
    case "set_default_cost_multiplier":
      return post<T>("/api/proxy/default-cost-multiplier/update", args);
    case "get_pricing_model_source":
      return post<T>("/api/proxy/pricing-model-source", args);
    case "set_pricing_model_source":
      return post<T>("/api/proxy/pricing-model-source/update", args);

    case "fetch_models_for_config":
      return post<T>("/api/models/fetch", args);

    case "get_usage_summary":
    case "get_usage_summary_by_app":
    case "get_usage_trends":
    case "get_provider_stats":
    case "get_model_stats":
    case "get_request_logs":
    case "get_request_detail":
    case "get_model_pricing":
    case "queryProviderUsage":
    case "testUsageScript":
    case "update_model_pricing":
    case "delete_model_pricing":
    case "check_provider_limits":
    case "sync_session_usage":
    case "get_usage_data_sources":
      return post<T>(`/api/usage/${command}`, args);

    case "get_universal_providers":
      return request<T>("/api/universal-providers");
    case "get_universal_provider":
      return request<T>(
        `/api/universal-providers/get?id=${encodeURIComponent(String(args.id))}`,
      );
    case "upsert_universal_provider":
      return post<T>("/api/universal-providers/upsert", args);
    case "delete_universal_provider":
      return post<T>("/api/universal-providers/delete", args);
    case "sync_universal_provider":
      return post<T>("/api/universal-providers/sync", args);

    case "get_webui_status":
      return request<T>("/api/webui/status");
    case "start_webui_server":
      return post<T>("/api/webui/start");
    case "stop_webui_server":
      return post<T>("/api/webui/stop");
    case "restart_webui_server":
      return post<T>("/api/webui/restart");

    case "get_init_error":
    case "get_migration_result":
    case "get_skills_migration_result":
      return null as T;
    case "set_window_theme":
      return true as T;

    default:
      throw new Error(
        `Command ${command} is not available in browser WebUI yet.`,
      );
  }
}

export async function listenEvent<T>(
  eventName: string,
  handler: EventHandler<T>,
): Promise<UnlistenFn> {
  if (isTauriRuntime()) {
    const { listen } = await import("@tauri-apps/api/event");
    return listen<T>(eventName, handler);
  }
  return () => {};
}

export const commandClient = {
  invoke: invokeCommand,
  listen: listenEvent,
};
