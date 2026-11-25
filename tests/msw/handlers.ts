import { http, HttpResponse } from "msw";
import type { AppId } from "@/lib/api/types";
import type { Provider, Settings } from "@/types";
import {
  addProvider,
  deleteProvider,
  deleteSession,
  getCurrentProviderId,
  getLiveProviderIds,
  getSessionMessages,
  getProviders,
  getWorkBuddyModelOwners,
  listProviders,
  listSessions,
  searchSessionContent,
  resetProviderState,
  setCurrentProviderId,
  ackPendingDeepLink,
  peekPendingDeepLink,
  updateProvider,
  updateSortOrder,
  getSettings,
  setSettings,
  getAppConfigDirOverride,
  setAppConfigDirOverrideState,
  listCustomIcons,
  addCustomIcon,
  renameCustomIcon,
  deleteCustomIcon,
} from "./state";

const TAURI_ENDPOINT = "http://tauri.local";

const withJson = async <T>(request: Request): Promise<T> => {
  try {
    const body = await request.text();
    if (!body) return {} as T;
    return JSON.parse(body) as T;
  } catch {
    return {} as T;
  }
};

const success = <T>(payload: T) => HttpResponse.json(payload as any);

export const handlers = [
  http.post(`${TAURI_ENDPOINT}/fetch_models_dev`, () =>
    success(JSON.stringify({})),
  ),
  http.post(`${TAURI_ENDPOINT}/get_migration_result`, () => success(false)),
  http.post(`${TAURI_ENDPOINT}/get_skills_migration_result`, () =>
    success(null),
  ),
  http.post(`${TAURI_ENDPOINT}/list_profiles`, () => success([])),
  http.post(`${TAURI_ENDPOINT}/get_providers`, async ({ request }) => {
    const { app } = await withJson<{ app: AppId }>(request);
    return success(getProviders(app));
  }),

  http.post(`${TAURI_ENDPOINT}/get_unreadable_providers`, () => success([])),
  http.post(`${TAURI_ENDPOINT}/get_unreadable_mcp_servers`, () => success([])),

  http.post(`${TAURI_ENDPOINT}/workbuddy_model_owners`, () =>
    success(getWorkBuddyModelOwners()),
  ),

  http.post(`${TAURI_ENDPOINT}/get_current_provider`, async ({ request }) => {
    const { app } = await withJson<{ app: AppId }>(request);
    return success(getCurrentProviderId(app));
  }),

  http.post(
    `${TAURI_ENDPOINT}/update_providers_sort_order`,
    async ({ request }) => {
      const { updates = [], app } = await withJson<{
        updates: { id: string; sortIndex: number }[];
        app: AppId;
      }>(request);
      updateSortOrder(app, updates);
      return success(true);
    },
  ),

  http.post(`${TAURI_ENDPOINT}/update_tray_menu`, () => success(true)),
  http.post(`${TAURI_ENDPOINT}/peek_pending_deeplink`, () =>
    success(peekPendingDeepLink()),
  ),
  // 默认下载失败：确认框退回内置图标或首字母
  http.post(`${TAURI_ENDPOINT}/fetch_deeplink_icon`, () => success(null)),
  http.post(`${TAURI_ENDPOINT}/ack_pending_deeplink`, async ({ request }) => {
    const { id } = await withJson<{ id: number }>(request);
    ackPendingDeepLink(id);
    return success(null);
  }),

  http.post(`${TAURI_ENDPOINT}/get_opencode_live_provider_ids`, () =>
    success(getLiveProviderIds("opencode")),
  ),

  http.post(`${TAURI_ENDPOINT}/get_opencode_default_model`, () =>
    success(null),
  ),

  http.post(`${TAURI_ENDPOINT}/set_opencode_default_model`, () =>
    success(null),
  ),

  http.post(`${TAURI_ENDPOINT}/get_openclaw_live_provider_ids`, () =>
    success(getLiveProviderIds("openclaw")),
  ),

  http.post(`${TAURI_ENDPOINT}/get_openclaw_default_model`, () =>
    success({ primary: null, fallback: [] }),
  ),

  http.post(`${TAURI_ENDPOINT}/scan_openclaw_config_health`, () => success([])),

  http.post(`${TAURI_ENDPOINT}/switch_provider`, async ({ request }) => {
    const { id, app } = await withJson<{ id: string; app: AppId }>(request);
    const providers = listProviders(app);
    if (!providers[id]) {
      return HttpResponse.json(false, { status: 404 });
    }
    setCurrentProviderId(app, id);
    return success(true);
  }),

  http.post(`${TAURI_ENDPOINT}/add_provider`, async ({ request }) => {
    const { provider, app } = await withJson<{
      provider: Provider & { id?: string };
      app: AppId;
    }>(request);

    const newId = provider.id ?? `mock-${Date.now()}`;
    addProvider(app, { ...provider, id: newId });
    return success(true);
  }),

  // 编辑器显示：测试里没有真实的 live 文件，关键字段直接取这一行（等于切到它之后的
  // 样子），不列出不生效的字段。
  http.post(
    `${TAURI_ENDPOINT}/get_provider_editor_view`,
    async ({ request }) => {
      const { settingsConfig = {} } = await withJson<{
        settingsConfig?: Record<string, unknown>;
      }>(request);
      return success({ settings: settingsConfig, inactive: [] });
    },
  ),

  http.post(`${TAURI_ENDPOINT}/update_provider`, async ({ request }) => {
    const { provider, app } = await withJson<{
      provider: Provider;
      app: AppId;
    }>(request);
    updateProvider(app, provider);
    return success(true);
  }),

  http.post(`${TAURI_ENDPOINT}/delete_provider`, async ({ request }) => {
    const { id, app } = await withJson<{ id: string; app: AppId }>(request);
    deleteProvider(app, id);
    return success(true);
  }),

  http.post(`${TAURI_ENDPOINT}/add_provider_to_live_config`, () =>
    success(true),
  ),

  http.post(`${TAURI_ENDPOINT}/remove_provider_from_live_config`, () =>
    success(true),
  ),

  http.post(`${TAURI_ENDPOINT}/import_default_config`, async () => {
    resetProviderState();
    return success(true);
  }),

  http.post(`${TAURI_ENDPOINT}/open_external`, () => success(true)),

  http.post(`${TAURI_ENDPOINT}/list_sessions`, () => success(listSessions())),

  http.post(`${TAURI_ENDPOINT}/sync_session_index`, () => {
    const total = listSessions().length;
    return success({ running: false, processed: total, total });
  }),

  http.post(`${TAURI_ENDPOINT}/search_session_content`, async ({ request }) => {
    const { query, providerIds } = await withJson<{
      query: string;
      providerIds?: string[];
    }>(request);
    const total = listSessions().length;
    return success({
      hits: searchSessionContent(query, providerIds),
      status: { running: false, processed: total, total },
    });
  }),

  http.post(`${TAURI_ENDPOINT}/cancel_session_content_search`, () =>
    success(null),
  ),
  http.post(`${TAURI_ENDPOINT}/clear_session_index`, () => success(true)),

  http.post(`${TAURI_ENDPOINT}/get_session_messages`, async ({ request }) => {
    const { providerId, sourcePath } = await withJson<{
      providerId: string;
      sourcePath: string;
    }>(request);
    return success(getSessionMessages(providerId, sourcePath));
  }),

  http.post(`${TAURI_ENDPOINT}/delete_session`, async ({ request }) => {
    const { providerId, sessionId, sourcePath } = await withJson<{
      providerId: string;
      sessionId: string;
      sourcePath: string;
    }>(request);
    return success(deleteSession(providerId, sessionId, sourcePath));
  }),

  http.post(`${TAURI_ENDPOINT}/delete_sessions`, async ({ request }) => {
    const { items = [] } = await withJson<{
      items?: {
        providerId: string;
        sessionId: string;
        sourcePath: string;
      }[];
    }>(request);

    return success(
      items.map((item) => ({
        providerId: item.providerId,
        sessionId: item.sessionId,
        sourcePath: item.sourcePath,
        success: deleteSession(
          item.providerId,
          item.sessionId,
          item.sourcePath,
        ),
      })),
    );
  }),

  // MCP APIs
  http.post(`${TAURI_ENDPOINT}/resync_mcp_to_apps`, async ({ request }) => {
    const { apps } = await withJson<{ apps?: AppId[] | null }>(request);
    const targets = apps?.length
      ? apps
      : [
          "claude",
          "codex",
          "gemini",
          "grokbuild",
          "opencode",
          "hermes",
          "mcode",
        ];
    return success(targets.map((app) => ({ app, ok: true })));
  }),

  // Skills / 用量：新加的只读或重投影命令，测试里默认都成功
  http.post(`${TAURI_ENDPOINT}/resync_skills_to_apps`, () => success([])),
  http.post(`${TAURI_ENDPOINT}/get_cc_switch_skills_dir`, () =>
    success("/home/test/.cc-switch/skills"),
  ),
  http.post(`${TAURI_ENDPOINT}/open_cc_switch_skills_dir`, () => success(true)),
  // 仓库访问令牌（#9666）：默认一个都没有，保存 / 删除原样回主机名
  http.post(`${TAURI_ENDPOINT}/get_skill_repo_tokens`, () =>
    success({ hosts: [], secureStorage: true }),
  ),
  http.post(`${TAURI_ENDPOINT}/set_skill_repo_token`, async ({ request }) => {
    const { host } = (await request.json()) as { host: string };
    return success(host);
  }),
  http.post(
    `${TAURI_ENDPOINT}/delete_skill_repo_token`,
    async ({ request }) => {
      const { host } = (await request.json()) as { host: string };
      return success(host);
    },
  ),
  http.post(`${TAURI_ENDPOINT}/get_session_usage_last_sync`, () =>
    success(null),
  ),

  http.post(`${TAURI_ENDPOINT}/restart_app`, () => success(true)),

  http.post(`${TAURI_ENDPOINT}/get_settings`, () => success(getSettings())),

  http.post(`${TAURI_ENDPOINT}/check_env_conflicts`, () => success([])),

  http.post(`${TAURI_ENDPOINT}/save_settings`, async ({ request }) => {
    const { settings } = await withJson<{ settings: Settings }>(request);
    setSettings(settings);
    return success(true);
  }),

  http.post(
    `${TAURI_ENDPOINT}/set_app_config_dir_override`,
    async ({ request }) => {
      const { path } = await withJson<{ path: string | null }>(request);
      setAppConfigDirOverrideState(path ?? null);
      return success(true);
    },
  ),

  http.post(`${TAURI_ENDPOINT}/get_app_config_dir_override`, () =>
    success(getAppConfigDirOverride()),
  ),

  http.post(
    `${TAURI_ENDPOINT}/apply_claude_plugin_config`,
    async ({ request }) => {
      const { official } = await withJson<{ official: boolean }>(request);
      setSettings({ enableClaudePluginIntegration: !official });
      return success(true);
    },
  ),

  http.post(`${TAURI_ENDPOINT}/apply_claude_onboarding_skip`, () =>
    success(true),
  ),

  http.post(`${TAURI_ENDPOINT}/clear_claude_onboarding_skip`, () =>
    success(true),
  ),

  http.post(`${TAURI_ENDPOINT}/get_config_dir`, async ({ request }) => {
    const { app } = await withJson<{ app: AppId }>(request);
    return success(app === "claude" ? "/default/claude" : "/default/codex");
  }),

  http.post(`${TAURI_ENDPOINT}/is_portable_mode`, () => success(false)),

  http.post(`${TAURI_ENDPOINT}/pick_directory`, async ({ request }) => {
    const { defaultPath, default_path } = await withJson<{
      defaultPath?: string;
      default_path?: string;
    }>(request);
    const initial = defaultPath ?? default_path;
    return success(initial ? `${initial}/picked` : "/mock/selected-dir");
  }),

  http.post(`${TAURI_ENDPOINT}/open_file_dialog`, () =>
    success("/mock/import-settings.json"),
  ),

  http.post(
    `${TAURI_ENDPOINT}/import_config_from_file`,
    async ({ request }) => {
      const { filePath } = await withJson<{ filePath: string }>(request);
      if (!filePath) {
        return success({ success: false, message: "Missing file" });
      }
      setSettings({ language: "en" });
      return success({ success: true, backupId: "backup-123" });
    },
  ),

  http.post(`${TAURI_ENDPOINT}/export_config_to_file`, async ({ request }) => {
    const { filePath } = await withJson<{ filePath: string }>(request);
    if (!filePath) {
      return success({ success: false, message: "Invalid destination" });
    }
    return success({ success: true, filePath });
  }),

  http.post(`${TAURI_ENDPOINT}/save_file_dialog`, () =>
    success("/mock/export-settings.json"),
  ),

  // Sync current providers live (no-op success)
  http.post(`${TAURI_ENDPOINT}/sync_current_providers_live`, () =>
    success({ success: true }),
  ),

  http.post(`${TAURI_ENDPOINT}/get_pi_current_state`, () =>
    success({
      enabledProviderIds: [],
      defaultProviderId: null,
    }),
  ),

  // Proxy status (for SettingsPage / ProxyPanel hooks)
  http.post(`${TAURI_ENDPOINT}/get_proxy_status`, () =>
    success({
      running: false,
      address: "127.0.0.1",
      port: 0,
      active_connections: 0,
      total_requests: 0,
      success_requests: 0,
      failed_requests: 0,
      success_rate: 0,
      uptime_seconds: 0,
      current_provider: null,
      current_provider_id: null,
      last_request_at: null,
      last_error: null,
      failover_count: 0,
      active_targets: [],
    }),
  ),

  http.post(`${TAURI_ENDPOINT}/get_proxy_stack`, () =>
    success({ active: false, members: [] }),
  ),

  http.post(`${TAURI_ENDPOINT}/set_proxy_stack_member`, () => success(null)),

  http.post(`${TAURI_ENDPOINT}/codex_forces_multi_agent_v2`, () =>
    success(false),
  ),

  http.post(`${TAURI_ENDPOINT}/restart_codex_app_server_daemon`, () =>
    success("restarted"),
  ),

  http.post(`${TAURI_ENDPOINT}/acknowledge_codex_stale_clients`, () =>
    success(null),
  ),

  http.post(`${TAURI_ENDPOINT}/get_proxy_takeover_status`, () =>
    success({
      claude: false,
      codex: false,
      gemini: false,
      grokbuild: false,
    }),
  ),

  // Failover / circuit breaker defaults
  http.post(`${TAURI_ENDPOINT}/get_auto_failover_enabled`, () =>
    success(false),
  ),
  http.post(`${TAURI_ENDPOINT}/get_failover_queue`, () => success([])),

  // 模式行：默认都在直连，直连那家就是当前供应商
  http.post(`${TAURI_ENDPOINT}/get_app_mode`, async ({ request }) => {
    const { appType } = await withJson<{ appType: AppId }>(request);
    return success({
      mode: "direct",
      attached: false,
      routeProviderId: null,
      directProviderId: getCurrentProviderId(appType) || null,
    });
  }),
  http.post(`${TAURI_ENDPOINT}/set_proxy_route`, () => success(null)),
  http.post(`${TAURI_ENDPOINT}/take_startup_attach_failures`, () =>
    success([]),
  ),
  http.post(`${TAURI_ENDPOINT}/take_tray_navigation`, () => success(null)),
  http.post(`${TAURI_ENDPOINT}/tray_app_page_seen`, () => success(null)),
  http.post(`${TAURI_ENDPOINT}/add_to_failover_queue`, () => success(true)),
  http.post(`${TAURI_ENDPOINT}/remove_from_failover_queue`, () =>
    success(true),
  ),

  http.post(`${TAURI_ENDPOINT}/get_provider_health`, () =>
    success({
      provider_id: "mock-provider",
      app_type: "claude",
      is_healthy: true,
      consecutive_failures: 0,
      last_success_at: null,
      last_failure_at: null,
      last_error: null,
      updated_at: new Date().toISOString(),
    }),
  ),

  // 侧栏：今日花费、授权中心提醒；「应用」页：Claude Desktop 接入状态
  http.post(`${TAURI_ENDPOINT}/get_usage_summary`, () =>
    success({
      totalRequests: 0,
      totalCost: "0",
      totalInputTokens: 0,
      totalOutputTokens: 0,
      totalCacheCreationTokens: 0,
      totalCacheReadTokens: 0,
      successRate: 0,
      realTotalTokens: 0,
      cacheHitRate: 0,
    }),
  ),
  // 会话阅读页头部：没有导入过用量时不显示
  http.post(`${TAURI_ENDPOINT}/get_session_usage_summary`, () =>
    success({
      totalRequests: 0,
      totalCost: "0",
      totalInputTokens: 0,
      totalOutputTokens: 0,
      totalCacheCreationTokens: 0,
      totalCacheReadTokens: 0,
      successRate: 0,
      realTotalTokens: 0,
      cacheHitRate: 0,
    }),
  ),
  http.post(`${TAURI_ENDPOINT}/auth_get_status`, async ({ request }) => {
    const { authProvider } = await withJson<{ authProvider: string }>(request);
    return success({
      provider: authProvider,
      authenticated: false,
      default_account_id: null,
      accounts: [],
    });
  }),
  http.post(`${TAURI_ENDPOINT}/claude_account_submit_paste`, () =>
    success(null),
  ),
  http.post(`${TAURI_ENDPOINT}/get_claude_login_switch`, () =>
    success({
      active: true,
      blocked: null,
      live: null,
    }),
  ),
  // 应用页页头的「提示词：X ›」会读各应用的提示词列表
  http.post(`${TAURI_ENDPOINT}/get_prompts`, () => success({})),
  http.post(
    `${TAURI_ENDPOINT}/get_prompt_file_location`,
    async ({ request }) => {
      const { app } = await withJson<{ app: AppId }>(request);
      const file =
        app === "claude"
          ? "CLAUDE.md"
          : app === "gemini"
            ? "GEMINI.md"
            : app === "hermes"
              ? "SOUL.md"
              : "AGENTS.md";
      return success({
        path: `/home/test/.${app}/${file}`,
        displayPath: `~/.${app}/${file}`,
      });
    },
  ),
  http.post(`${TAURI_ENDPOINT}/get_claude_desktop_status`, () =>
    success({
      supported: true,
      installed: false,
      configured: false,
      proxyRunning: false,
      staleRawModels: false,
      missingRouteMappings: false,
      gatewayTokenConfigured: false,
    }),
  ),
  http.post(`${TAURI_ENDPOINT}/get_hermes_memory`, async ({ request }) => {
    const { kind } = await withJson<{ kind: string }>(request);
    return success(kind === "user" ? "user profile" : "agent notes");
  }),
  http.post(`${TAURI_ENDPOINT}/get_hermes_memory_limits`, () =>
    success({
      memory: 2200,
      user: 1375,
      memoryEnabled: true,
      userEnabled: true,
    }),
  ),
  http.post(`${TAURI_ENDPOINT}/get_custom_icons`, () =>
    success(listCustomIcons()),
  ),
  http.post(`${TAURI_ENDPOINT}/add_custom_icon`, async ({ request }) => {
    const { name, data } = await withJson<{ name: string; data: string }>(
      request,
    );
    return success(addCustomIcon(name, data));
  }),
  http.post(`${TAURI_ENDPOINT}/rename_custom_icon`, async ({ request }) => {
    const { id, name } = await withJson<{ id: string; name: string }>(request);
    return success(renameCustomIcon(id, name));
  }),
  http.post(`${TAURI_ENDPOINT}/delete_custom_icon`, async ({ request }) => {
    const { id } = await withJson<{ id: string }>(request);
    deleteCustomIcon(id);
    return success(null);
  }),

  // Read on mount by settings, provider and account components. Values are what
  // a fresh install with nothing signed in or configured returns.
  http.post(`${TAURI_ENDPOINT}/get_privacy_filter_config`, () =>
    success({ enabled: false, filterToolResults: false }),
  ),
  http.post(`${TAURI_ENDPOINT}/get_codex_session_compression`, () =>
    success(false),
  ),
  http.post(`${TAURI_ENDPOINT}/get_codex_sessions_disk_usage`, () =>
    success(0),
  ),
  http.post(`${TAURI_ENDPOINT}/get_global_proxy_url`, () => success(null)),
  http.post(
    `${TAURI_ENDPOINT}/get_proxy_config_for_app`,
    async ({ request }) => {
      const { appType } = await withJson<{ appType: string }>(request);
      // proxy_config column defaults (database/schema.rs)
      return success({
        appType,
        enabled: false,
        autoFailoverEnabled: false,
        maxRetries: 3,
        streamingFirstByteTimeout: 60,
        streamingIdleTimeout: 120,
        nonStreamingTimeout: 600,
        circuitFailureThreshold: 4,
        circuitSuccessThreshold: 2,
        circuitTimeoutSeconds: 60,
        circuitErrorRateThreshold: 0.6,
        circuitMinRequests: 10,
      });
    },
  ),
  http.post(`${TAURI_ENDPOINT}/get_hermes_model_config`, () => success(null)),
  http.post(`${TAURI_ENDPOINT}/get_openclaw_env`, () => success({})),
  http.post(`${TAURI_ENDPOINT}/get_current_omo_provider_id`, () => success("")),
  http.post(`${TAURI_ENDPOINT}/get_current_omo_slim_provider_id`, () =>
    success(""),
  ),
  http.post(`${TAURI_ENDPOINT}/get_claude_default_plan`, () => success(null)),
  http.post(`${TAURI_ENDPOINT}/dsh_account_status`, () =>
    success({
      signedIn: false,
      expired: false,
      issuer: null,
      profile: null,
      wallets: [],
      bonusWallets: [],
      usageUrl: null,
      topUpUrl: null,
      error: null,
    }),
  ),
  http.post(`${TAURI_ENDPOINT}/workbuddy_account_live_state`, () =>
    success({
      running: false,
      signedIn: false,
      recognized: false,
      imported: false,
    }),
  ),
  http.post(`${TAURI_ENDPOINT}/workbuddy_account_quota`, () =>
    success({
      snapshot: null,
      current: false,
      unavailable: "not_running",
      error: null,
    }),
  ),
  // 授权中心的 CodeBuddy 分区一打开就读本机登录状态：默认没装 / 没登录
  http.post(`${TAURI_ENDPOINT}/codebuddy_account_live`, () =>
    success({
      state: "none",
      account_id: null,
      label: null,
      domain: null,
    }),
  ),
];
