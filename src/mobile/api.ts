/**
 * Pure browser fetch client for the CC Switch LAN Mobile Web UI.
 * Uses HttpOnly SameSite=Strict session cookies (`credentials: "same-origin"`).
 */

export interface AuthStatus {
  authenticated: boolean;
  allowControl: boolean;
  remoteControlApps?: string[];
  version: string;
}

export interface UsageSummary {
  totalRequests: number;
  totalCost: string;
  totalInputTokens: number;
  totalOutputTokens: number;
  totalCacheCreationTokens: number;
  totalCacheReadTokens: number;
  successRate: number;
  realTotalTokens: number;
  cacheHitRate: number;
}

export interface DailyStats {
  date: string;
  requestCount: number;
  totalCost: string;
  totalTokens: number;
  totalInputTokens: number;
  totalOutputTokens: number;
  totalCacheCreationTokens: number;
  totalCacheReadTokens: number;
}

export interface ProviderStats {
  providerId: string;
  appType: string;
  providerName: string;
  requestCount: number;
  totalTokens: number;
  totalCost: string;
  successRate: number;
  avgLatencyMs: number;
  cacheReadTokens: number;
  cacheableInputTokens: number;
  speedOutputTokens: number;
  speedGenerationMs: number;
  estSpeedOutputTokens: number;
  estSpeedDurationMs: number;
  firstTokenMedianMs: number | null;
  estFirstTokenMedianMs: number | null;
}

export interface ModelStats {
  model: string;
  requestCount: number;
  totalTokens: number;
  totalCost: string;
  avgCostPerRequest: string;
  successRate: number;
  cacheReadTokens: number;
  cacheableInputTokens: number;
  speedOutputTokens: number;
  speedGenerationMs: number;
  estSpeedOutputTokens: number;
  estSpeedDurationMs: number;
  firstTokenMedianMs: number | null;
  estFirstTokenMedianMs: number | null;
}

export interface ActiveAppSnapshot {
  app: string;
  currentProviderId: string | null;
  currentProviderName: string | null;
  providerCount: number;
}

export interface ActiveTarget {
  appType: string;
  providerName: string;
  providerId: string;
}

export interface ProxyStatus {
  running: boolean;
  address: string;
  port: number;
  activeConnections: number;
  totalRequests: number;
  successRequests: number;
  failedRequests: number;
  successRate: number;
  uptimeSeconds: number;
  currentProvider: string | null;
  currentProviderId: string | null;
  lastRequestAt: string | null;
  lastError: string | null;
  failoverCount: number;
  activeTargets: ActiveTarget[];
}

export interface ProxyTakeoverStatus {
  claude: boolean;
  codex: boolean;
  gemini: boolean;
  grokbuild: boolean;
  "claude-desktop": boolean;
}

export interface OverviewResponse {
  days: number;
  summary: UsageSummary;
  dailyTrends: DailyStats[];
  providerStats: ProviderStats[];
  modelStats: ModelStats[];
  aggregateTps: number | null;
  aggregateTpsEstimated: boolean;
  activeApps: ActiveAppSnapshot[];
  proxyStatus: ProxyStatus;
  allowControl: boolean;
}

export interface SafeProviderHealth {
  isHealthy: boolean;
  consecutiveFailures: number;
  lastError: string | null;
  updatedAt: string;
}

export interface SafeProvider {
  id: string;
  name: string;
  category?: string;
  icon?: string;
  iconColor?: string;
  websiteUrl?: string;
  isCurrent: boolean;
  inFailoverQueue: boolean;
  health?: SafeProviderHealth;
}

export interface SafeAppProviders {
  app: string;
  currentProviderId: string | null;
  providers: SafeProvider[];
}

export interface ProvidersListResponse {
  allowControl: boolean;
  apps: SafeAppProviders[];
}

export interface SafeRequestLog {
  requestId: string;
  providerId: string;
  providerName?: string;
  appType: string;
  model: string;
  inputTokens: number;
  outputTokens: number;
  cacheReadTokens: number;
  cacheCreationTokens: number;
  totalCostUsd: string;
  isStreaming: boolean;
  latencyMs: number;
  firstTokenMs: number | null;
  durationMs: number | null;
  tps: number | null;
  tpsEstimated: boolean;
  statusCode: number;
  errorMessage: string | null;
  createdAt: number;
  dataSource?: string;
}

export interface SafePaginatedLogs {
  data: SafeRequestLog[];
  total: number;
  page: number;
  pageSize: number;
}

export interface ProxyStatusApiResponse {
  status: ProxyStatus;
  takeover: ProxyTakeoverStatus;
  allowControl: boolean;
}

export type RemoteRunStatus = "running" | "completed" | "error" | "cancelled";

export type RemoteEventKind =
  | "started"
  | "init"
  | "thought"
  | "text"
  | "tool_call"
  | "tool_result"
  | "stderr"
  | "completed"
  | "error"
  | "cancelled";

export interface RemoteAgentInfo {
  id: string;
  installed: boolean;
  enabled: boolean;
  supportsResume: boolean;
  supportsModel: boolean;
  supportsEffort: boolean;
  supportsMode: boolean;
  supportsSandbox: boolean;
  modes: string[];
  efforts: string[];
  activeRunCount: number;
}

export interface RemoteAgentsResponse {
  agents: RemoteAgentInfo[];
  defaultCwd: string;
  recentWorkspaces: string[];
}

export interface SafeRemoteSession {
  app: string;
  sessionId: string;
  title?: string;
  summary?: string;
  projectDir?: string;
  createdAt?: number;
  lastActiveAt?: number;
  resumable: boolean;
  activeRunId?: string;
}

export interface RemoteSessionsResponse {
  sessions: SafeRemoteSession[];
}

export interface RemoteSessionBlock {
  type: string;
  text?: string;
  summary?: string;
  redacted?: boolean;
  rawName?: string;
  title?: string;
  detail?: string;
  inputPreview?: string;
  preview?: string;
  status?: string;
  kind?: string;
}

export interface RemoteSessionMessage {
  role: string;
  content?: string;
  ts?: number;
  id?: string;
  injected?: boolean;
  blocks?: RemoteSessionBlock[];
}

export interface RemoteSessionMessagesResponse {
  session: SafeRemoteSession;
  messages: RemoteSessionMessage[];
}

export interface RemoteStreamEvent {
  seq: number;
  ts: number;
  kind: RemoteEventKind;
  text?: string;
  toolName?: string;
  toolId?: string;
  isError?: boolean;
  sessionId?: string;
  exitCode?: number;
}

export interface RemoteRunSnapshot {
  runId: string;
  app: string;
  status: RemoteRunStatus;
  prompt: string;
  sessionId?: string;
  conversationId?: string;
  cwd: string;
  model?: string;
  effort?: string;
  mode?: string;
  sandbox: boolean;
  createdAt: number;
  updatedAt: number;
  finishedAt?: number;
  exitCode?: number;
  error?: string;
  lastSeq: number;
}

export interface RemoteRunsResponse {
  activeRunCount: number;
  runs: RemoteRunSnapshot[];
}

export interface RemoteRunDetailResponse {
  run: RemoteRunSnapshot;
  events: RemoteStreamEvent[];
}

export interface StartRemoteRunRequest {
  app: string;
  prompt: string;
  sessionId?: string;
  cwd?: string;
  model?: string;
  effort?: string;
  mode?: string;
  sandbox?: boolean;
}

export class ApiError extends Error {
  status: number;
  constructor(status: number, message: string) {
    super(message);
    this.status = status;
  }
}

async function requestJson<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(path, {
    credentials: "same-origin",
    headers: {
      "Content-Type": "application/json",
      ...(init?.headers ?? {}),
    },
    ...init,
  });
  const text = await res.text();
  let parsed: unknown = null;
  try {
    parsed = text ? JSON.parse(text) : null;
  } catch {
    // non-json error body
  }
  if (!res.ok) {
    let msg = `${res.status} ${res.statusText}`;
    if (
      parsed &&
      typeof parsed === "object" &&
      "error" in parsed &&
      typeof (parsed as { error: unknown }).error === "string"
    ) {
      msg = (parsed as { error: string }).error;
    }
    throw new ApiError(res.status, msg);
  }
  return parsed as T;
}

/** Extract token from URL fragment (`#t=<token>`) and strip it immediately. */
export function consumeFragmentToken(): string | null {
  if (typeof window === "undefined") return null;
  const hash = window.location.hash;
  if (!hash || hash.length < 2) return null;
  const params = new URLSearchParams(hash.slice(1));
  const token = params.get("t");
  if (!token) return null;
  try {
    window.history.replaceState(
      null,
      "",
      window.location.pathname + window.location.search,
    );
  } catch {
    // ignore history restrictions
  }
  return token.trim() || null;
}

export const mobileApi = {
  getAuthStatus: () => requestJson<AuthStatus>("/api/auth/status"),

  createSession: (token: string) =>
    requestJson<AuthStatus>("/api/auth/session", {
      method: "POST",
      body: JSON.stringify({ token }),
    }),

  logout: () =>
    requestJson<{ authenticated: boolean }>("/api/auth/logout", {
      method: "POST",
    }),

  getOverview: (days: 1 | 7 | 30) =>
    requestJson<OverviewResponse>(`/api/overview?days=${days}`),

  getProviders: () => requestJson<ProvidersListResponse>("/api/providers"),

  switchProvider: (app: string, id: string) =>
    requestJson<{ ok: boolean; warnings: string[] }>(
      `/api/providers/${encodeURIComponent(app)}/${encodeURIComponent(id)}/switch`,
      { method: "POST" },
    ),

  getLogs: (params: {
    page: number;
    pageSize: number;
    app?: string;
    status?: number;
    statusClass?: "ok" | "err";
  }) => {
    const q = new URLSearchParams({
      page: String(params.page),
      pageSize: String(params.pageSize),
    });
    if (params.app) q.set("app", params.app);
    if (params.status) q.set("status", String(params.status));
    if (params.statusClass) q.set("statusClass", params.statusClass);
    return requestJson<SafePaginatedLogs>(`/api/logs?${q.toString()}`);
  },

  getProxyStatus: () =>
    requestJson<ProxyStatusApiResponse>("/api/proxy/status"),

  startProxy: () =>
    requestJson<{
      ok: boolean;
      status: ProxyStatus;
      takeover: ProxyTakeoverStatus;
    }>("/api/proxy/start", { method: "POST" }),

  stopProxy: () =>
    requestJson<{
      ok: boolean;
      status: ProxyStatus;
      takeover: ProxyTakeoverStatus;
    }>("/api/proxy/stop", { method: "POST" }),

  getRemoteAgents: () =>
    requestJson<RemoteAgentsResponse>("/api/remote/agents"),

  getRemoteSessions: (params?: {
    app?: string;
    limit?: number;
    q?: string;
  }) => {
    const q = new URLSearchParams();
    if (params?.app && params.app !== "all") q.set("app", params.app);
    if (params?.limit) q.set("limit", String(params.limit));
    if (params?.q?.trim()) q.set("q", params.q.trim());
    const qs = q.toString();
    return requestJson<RemoteSessionsResponse>(
      `/api/remote/sessions${qs ? `?${qs}` : ""}`,
    );
  },

  getRemoteSessionMessages: (app: string, sessionId: string) =>
    requestJson<RemoteSessionMessagesResponse>(
      `/api/remote/sessions/${encodeURIComponent(app)}/${encodeURIComponent(sessionId)}/messages`,
    ),

  getRemoteRuns: (app?: string) => {
    const qs = app && app !== "all" ? `?app=${encodeURIComponent(app)}` : "";
    return requestJson<RemoteRunsResponse>(`/api/remote/runs${qs}`);
  },

  startRemoteRun: (payload: StartRemoteRunRequest) =>
    requestJson<RemoteRunSnapshot>("/api/remote/runs", {
      method: "POST",
      body: JSON.stringify(payload),
    }),

  getRemoteRun: (runId: string, after = 0) =>
    requestJson<RemoteRunDetailResponse>(
      `/api/remote/runs/${encodeURIComponent(runId)}?after=${after}`,
    ),

  cancelRemoteRun: (runId: string) =>
    requestJson<RemoteRunSnapshot>(
      `/api/remote/runs/${encodeURIComponent(runId)}/cancel`,
      { method: "POST" },
    ),

  remoteRunStreamUrl: (runId: string, after = 0) =>
    `/api/remote/runs/${encodeURIComponent(runId)}/stream?after=${after}`,
};
