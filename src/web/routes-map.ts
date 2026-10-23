/**
 * command → REST 路由映射表（parity 契约）。
 *
 * shim 的 `invoke(cmd, args)` 经此表派发 fetch；`unwrap` 规则把 REST 返回归一化为
 * Tauri command 的返回形状。未列出的命令 → `UnsupportedInWebMode`。
 *
 * 解包规则：
 * - undefined  → passthrough（REST 返回即 command 返回）
 * - "ok"       → data.ok（command 返 bool 的 mutation）
 * - "okNull"   → null（command 返 () 的 mutation）
 * - "current"  → data.current
 * - "url"      → data.url
 * - "enabled"  → data.enabled
 */

export type Unwrap =
  | "ok"
  | "okNull"
  | "current"
  | "url"
  | "enabled"
  | undefined;

export interface Route {
  method: "GET" | "POST" | "PUT" | "PATCH" | "DELETE";
  /** 由 invoke args 构造路径（含 query）。`:id`/`:app`/`:scope` 从 args 填入。 */
  path: (a: any) => string;
  /** 非 GET 时构造 body（camelCase 字段名已与 REST serde rename 对齐）。 */
  body?: (a: any) => unknown;
  unwrap?: Unwrap;
}

const B = "/control/v1";

/// 构造 query string，丢弃 null/undefined（匹配 axum Option 字段缺省 → None）。
function qs(obj: Record<string, unknown>): string {
  const e = Object.entries(obj).filter(([, v]) => v != null && v !== undefined);
  return e.length ? `?${new URLSearchParams(e as any)}` : "";
}

export const ROUTES: Record<string, Route> = {
  // ───────── providers ─────────
  get_providers: { method: "GET", path: (a) => `${B}/providers?app=${a.app}` },
  get_settings: { method: "GET", path: () => `${B}/settings` },
  get_config_dir: {
    method: "GET",
    path: (a) => `${B}/config-dir?app=${a.app}`,
  },
  get_app_config_dir_override: {
    method: "GET",
    path: () => `${B}/app-config-dir-override`,
  },
  get_installed_skills: { method: "GET", path: () => `${B}/installed-skills` },
  get_migration_result: { method: "GET", path: () => `${B}/migration-result` },
  get_skills_migration_result: {
    method: "GET",
    path: () => `${B}/skills-migration-result`,
  },
  get_rest_config: { method: "GET", path: () => `${B}/rest-config` },
  get_pi_current_state: { method: "GET", path: () => `${B}/pi-current-state` },
  get_current_provider: {
    method: "GET",
    path: (a) => `${B}/providers/current?app=${a.app}`,
  },
  switch_provider: {
    method: "POST",
    path: () => `${B}/providers/switch`,
    body: (a) => ({ app: a.app, provider_id: a.id }),
  },
  save_settings: {
    method: "PUT",
    path: () => `${B}/settings`,
    body: (a) => ({ settings: a.settings }),
    unwrap: "ok",
  },
  add_provider: {
    method: "POST",
    path: () => `${B}/providers`,
    body: (a) => a,
    unwrap: "ok",
  },
  update_provider: {
    method: "PUT",
    path: (a) => `${B}/providers/${a.originalId}`,
    body: (a) => ({
      app: a.app,
      provider: a.provider,
      editorSave: a.editorSave,
    }),
    unwrap: "ok",
  },
  delete_provider: {
    method: "DELETE",
    path: (a) => `${B}/providers/${a.id}?app=${a.app}`,
    unwrap: "ok",
  },
  get_provider_editor_view: {
    method: "POST",
    path: () => `${B}/providers/editor-view`,
    body: (a) => a,
  },
  test_api_endpoints: {
    method: "POST",
    path: () => `${B}/providers/test-endpoints`,
    body: (a) => ({ urls: a.urls, timeoutSecs: a.timeoutSecs }),
  },
  get_custom_endpoints: {
    method: "GET",
    path: (a) => `${B}/providers/${a.providerId}/custom-endpoints?app=${a.app}`,
  },
  add_custom_endpoint: {
    method: "POST",
    path: (a) => `${B}/providers/${a.providerId}/custom-endpoints`,
    body: (a) => ({ app: a.app, url: a.url }),
    unwrap: "okNull",
  },
  remove_custom_endpoint: {
    method: "DELETE",
    path: (a) => `${B}/providers/${a.providerId}/custom-endpoints`,
    body: (a) => ({ app: a.app, url: a.url }),
    unwrap: "okNull",
  },
  update_providers_sort_order: {
    method: "PATCH",
    path: () => `${B}/providers/sort`,
    body: (a) => ({ app: a.app, updates: a.updates }),
    unwrap: "ok",
  },

  // ───────── proxy ─────────
  get_proxy_status: { method: "GET", path: () => `${B}/proxy/status` },
  // 整体 ProxyConfig（command get_proxy_config；注意非 per-app）
  // 注：前端实际调 get_proxy_config_for_app（见下），此处保留以对齐 REST。
  // ─ per-app（前端实际调用）─
  get_proxy_config_for_app: {
    method: "GET",
    path: (a) => `${B}/proxy/config/${a.appType}`,
  },
  update_proxy_config_for_app: {
    method: "PUT",
    path: (a) => `${B}/proxy/config/${a.config?.appType ?? a.appType}`,
    body: (a) => a.config,
    unwrap: "okNull",
  },
  get_proxy_takeover_status: {
    method: "GET",
    path: () => `${B}/proxy/takeover`,
  },
  set_proxy_takeover_for_app: {
    method: "POST",
    path: (a) => `${B}/proxy/takeover/${a.appType}?enabled=${a.enabled}`,
    unwrap: "okNull",
  },
  get_provider_health: {
    method: "GET",
    path: (a) => `${B}/proxy/health/${a.appType}?providerId=${a.providerId}`,
  },
  get_circuit_breaker_config: {
    method: "GET",
    path: () => `${B}/proxy/circuit-breaker/config`,
  },
  update_circuit_breaker_config: {
    method: "PUT",
    path: () => `${B}/proxy/circuit-breaker/config`,
    body: (a) => a.config,
    unwrap: "okNull",
  },
  get_circuit_breaker_stats: {
    method: "GET",
    path: (a) =>
      `${B}/proxy/circuit-breaker/stats?provider_id=${a.providerId}&app=${a.appType}`,
  },
  reset_circuit_breaker: {
    method: "POST",
    path: () => `${B}/proxy/circuit-breaker/reset`,
    body: (a) => ({ providerId: a.providerId, appType: a.appType }),
    unwrap: "okNull",
  },

  // ───────── global-proxy ─────────
  get_global_proxy_url: {
    method: "GET",
    path: () => `${B}/global-proxy/url`,
    unwrap: "url",
  },
  set_global_proxy_url: {
    method: "PUT",
    path: () => `${B}/global-proxy/url`,
    body: (a) => ({ url: a.url }),
    unwrap: "okNull",
  },
  test_proxy_url: {
    method: "POST",
    path: () => `${B}/global-proxy/test`,
    body: (a) => ({ url: a.url }),
  },
  scan_local_proxies: { method: "POST", path: () => `${B}/global-proxy/scan` },
  get_global_proxy_config: {
    method: "GET",
    path: () => `${B}/global-proxy/config`,
  },
  update_global_proxy_config: {
    method: "PUT",
    path: () => `${B}/global-proxy/config`,
    body: (a) => a.config,
    unwrap: "okNull",
  },
  scan_openclaw_config_health: {
    method: "POST",
    path: () => `${B}/global-proxy/scan`,
  },

  // ───────── failover ─────────
  get_failover_queue: {
    method: "GET",
    path: (a) => `${B}/failover/queue?app=${a.appType}`,
  },
  get_available_providers_for_failover: {
    method: "GET",
    path: (a) => `${B}/failover/available?app=${a.appType}`,
  },
  add_to_failover_queue: {
    method: "POST",
    path: () => `${B}/failover/queue`,
    body: (a) => ({ appType: a.appType, providerId: a.providerId }),
  },
  remove_from_failover_queue: {
    method: "DELETE",
    path: (a) => `${B}/failover/queue/${a.providerId}?app=${a.appType}`,
  },
  get_auto_failover_enabled: {
    method: "GET",
    path: (a) => `${B}/failover/auto?app=${a.appType}`,
  },
  set_auto_failover_enabled: {
    method: "PUT",
    path: () => `${B}/failover/auto`,
    body: (a) => ({ appType: a.appType, enabled: a.enabled }),
    unwrap: "okNull",
  },

  // ───────── profile ─────────
  list_profiles: { method: "GET", path: () => `${B}/profiles` },
  create_profile: {
    method: "POST",
    path: () => `${B}/profiles`,
    body: (a) => ({ name: a.name, scope: a.scope }),
  },
  update_profile: {
    method: "PUT",
    path: (a) => `${B}/profiles/${a.id}`,
    body: (a) => ({ name: a.name, resnapshot: a.resnapshot, scope: a.scope }),
  },
  delete_profile: {
    method: "DELETE",
    path: (a) => `${B}/profiles/${a.id}`,
    unwrap: "okNull",
  },
  apply_profile: {
    method: "POST",
    path: (a) => `${B}/profiles/${a.id}/apply?scope=${a.scope}`,
  },
  clear_current_profile: {
    method: "DELETE",
    path: (a) => `${B}/profiles/current/${a.scope}`,
    unwrap: "okNull",
  },

  // ───────── lightweight / portable / balance ─────────
  is_lightweight_mode: { method: "GET", path: () => `${B}/lightweight-mode` },
  is_portable_mode: { method: "GET", path: () => `${B}/portable-mode` },
  get_balance: {
    method: "GET",
    path: (a) =>
      `${B}/balance?baseUrl=${encodeURIComponent(a.baseUrl)}&apiKey=${encodeURIComponent(a.apiKey)}`,
  },

  // ───────── usage stats ─────────
  get_usage_summary: {
    method: "GET",
    path: (a) =>
      `${B}/usage/summary${qs({ startDate: a.startDate, endDate: a.endDate, appType: a.appType, providerName: a.providerName, model: a.model })}`,
  },
  get_usage_summary_by_app: {
    method: "GET",
    path: (a) =>
      `${B}/usage/summary-by-app${qs({ startDate: a.startDate, endDate: a.endDate, providerName: a.providerName, model: a.model })}`,
  },
  get_usage_trends: {
    method: "GET",
    path: (a) =>
      `${B}/usage/trends${qs({ startDate: a.startDate, endDate: a.endDate, appType: a.appType, providerName: a.providerName, model: a.model })}`,
  },
  // 注：get_provider_stats / get_model_stats / get_request_logs 已迁 #[command_api]，
  // 由 GET /control/v1/__routes 发现端点自动路由（shim lazy-fetch 建表）——
  // 零配置迁移命令无需手写 routes-map 条目。

  // get_tool_versions — #[command_api] POST，body camelCase passthrough
  // 显式映射确保关于页工具版本探测不被 auto-route 首次 fetch 失败拖累
  get_tool_versions: {
    method: "POST",
    path: () => `${B}/get_tool_versions`,
    body: (a) => a,
  },
};

/** 应用解包规则，使返回形状 = Tauri command 返回。 */
export function applyUnwrap(data: any, rule: Unwrap): any {
  switch (rule) {
    case "ok":
      return data?.ok;
    case "okNull":
      return null;
    case "current":
      return data?.current;
    case "url":
      return data?.url;
    case "enabled":
      return data?.enabled;
    default:
      return data;
  }
}
