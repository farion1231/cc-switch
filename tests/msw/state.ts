import type { AppId } from "@/lib/api/types";
import type {
  McpServer,
  Provider,
  SessionMessage,
  SessionMeta,
  Settings,
} from "@/types";
import type {
  AgentSessionNodeView,
  AgentSessionUsageRequest,
  AgentSessionUsageSummary,
  AgentTaskUsageFilter,
  AgentTaskUsageFilterOptions,
  AgentTaskUsageFilterOptionsRequest,
  AgentTaskUsagePage,
  AgentTaskUsageRow,
  AgentUsageCapability,
  AgentUsageMeasure,
  AgentUsageSourceDimension,
} from "@/types/usage";
import { deepClone } from "@/utils/deepClone";
import {
  createAgentSessionNode,
  createAgentSessionUsageSummary as fixtureSessionSummary,
  createAgentUsageCapability as fixtureCapability,
  createAgentUsageMeasure,
  createAgentUsageSourceDimension as fixtureDimension,
} from "../fixtures/agentUsage";

type ProvidersByApp = Record<AppId, Record<string, Provider>>;
type CurrentProviderState = Record<AppId, string>;
type McpConfigState = Record<AppId, Record<string, McpServer>>;
type LiveProviderIdsByApp = Record<
  "opencode" | "openclaw" | "hermes",
  string[]
>;

const createDefaultProviders = (): ProvidersByApp => ({
  claude: {
    "claude-1": {
      id: "claude-1",
      name: "Claude Default",
      settingsConfig: {},
      category: "official",
      sortIndex: 0,
      createdAt: Date.now(),
    },
    "claude-2": {
      id: "claude-2",
      name: "Claude Custom",
      settingsConfig: {},
      category: "custom",
      sortIndex: 1,
      createdAt: Date.now() + 1,
    },
  },
  "claude-desktop": {},
  codex: {
    "codex-1": {
      id: "codex-1",
      name: "Codex Default",
      settingsConfig: {},
      category: "official",
      sortIndex: 0,
      createdAt: Date.now(),
    },
    "codex-2": {
      id: "codex-2",
      name: "Codex Secondary",
      settingsConfig: {},
      category: "custom",
      sortIndex: 1,
      createdAt: Date.now() + 1,
    },
  },
  gemini: {
    "gemini-1": {
      id: "gemini-1",
      name: "Gemini Default",
      settingsConfig: {
        env: {
          GEMINI_API_KEY: "test-key",
          GOOGLE_GEMINI_BASE_URL: "https://generativelanguage.googleapis.com",
        },
      },
      category: "official",
      sortIndex: 0,
      createdAt: Date.now(),
    },
  },
  grokbuild: {},
  opencode: {},
  openclaw: {},
  hermes: {},
  pi: {},
});

const createDefaultCurrent = (): CurrentProviderState => ({
  claude: "claude-1",
  "claude-desktop": "",
  codex: "codex-1",
  gemini: "gemini-1",
  grokbuild: "",
  opencode: "",
  openclaw: "",
  hermes: "",
  pi: "",
});

let providers = createDefaultProviders();
let current = createDefaultCurrent();
let liveProviderIds: LiveProviderIdsByApp = {
  opencode: [],
  openclaw: [],
  hermes: [],
};
let settingsState: Settings = {
  showInTray: true,
  minimizeToTrayOnClose: true,
  enableClaudePluginIntegration: false,
  claudeConfigDir: "/default/claude",
  codexConfigDir: "/default/codex",
  language: "zh",
};
let appConfigDirOverride: string | null = null;
const sessionMessageKey = (providerId: string, sourcePath: string) =>
  `${providerId}:${sourcePath}`;

const createDefaultSessions = (): SessionMeta[] => {
  const now = Date.now();
  return [
    {
      providerId: "codex",
      sessionId: "codex-session-1",
      title: "Codex Session One",
      summary: "Codex summary",
      projectDir: "/mock/codex",
      createdAt: now - 2000,
      lastActiveAt: now - 1000,
      sourcePath: "/mock/codex/session-1.jsonl",
      resumeCommand: "codex resume codex-session-1",
    },
    {
      providerId: "claude",
      sessionId: "claude-session-1",
      title: "Claude Session One",
      summary: "Claude summary",
      projectDir: "/mock/claude",
      createdAt: now - 4000,
      lastActiveAt: now - 3000,
      sourcePath: "/mock/claude/session-1.jsonl",
      resumeCommand: "claude --resume claude-session-1",
    },
  ];
};

const createDefaultSessionMessages = (): Record<string, SessionMessage[]> => ({
  [sessionMessageKey("codex", "/mock/codex/session-1.jsonl")]: [
    {
      role: "user",
      content: "First codex message",
      ts: Date.now() - 1000,
    },
  ],
  [sessionMessageKey("claude", "/mock/claude/session-1.jsonl")]: [
    {
      role: "user",
      content: "First claude message",
      ts: Date.now() - 3000,
    },
  ],
});

let sessionsState = createDefaultSessions();
let sessionMessagesState = createDefaultSessionMessages();
let mcpConfigs: McpConfigState = {
  claude: {
    sample: {
      id: "sample",
      name: "Sample Claude Server",
      enabled: true,
      apps: {
        claude: true,
        codex: false,
        gemini: false,
        opencode: false,
        openclaw: false,
        hermes: false,
      },
      server: {
        type: "stdio",
        command: "claude-server",
      },
    },
  },
  "claude-desktop": {},
  codex: {
    httpServer: {
      id: "httpServer",
      name: "HTTP Codex Server",
      enabled: false,
      apps: {
        claude: false,
        codex: true,
        gemini: false,
        opencode: false,
        openclaw: false,
        hermes: false,
      },
      server: {
        type: "http",
        url: "http://localhost:3000",
      },
    },
  },
  gemini: {},
  grokbuild: {},
  opencode: {},
  openclaw: {},
  hermes: {},
  pi: {},
};

type AgentUsageFixtureState = {
  capabilities: AgentUsageCapability[];
  summaries: Record<string, AgentSessionUsageSummary>;
  tasks: AgentTaskUsageRow[];
};

const agentUsageFixtureKey = (appType: string, sessionId: string) =>
  `${appType}:${sessionId}`;

const fixtureMeasure = (
  dataSource: string,
  overrides: Partial<AgentUsageMeasure> = {},
): AgentUsageMeasure => createAgentUsageMeasure({ dataSource, ...overrides });

// Keep this alias local to the anonymous fixture helpers so no production
// app list is duplicated or exported from the MSW state module.
type AgentUsageNodeAppType = AgentSessionNodeView["appType"];

const fixtureNode = (
  appType: AgentUsageNodeAppType,
  sessionId: string,
  title: string,
  projectDir: string,
): AgentSessionNodeView =>
  createAgentSessionNode(appType, sessionId, { title, projectDir });

const fixtureSummary = (
  appType: AgentUsageNodeAppType,
  sessionId: string,
  measure: AgentUsageMeasure | null,
  options: {
    title: string;
    projectDir: string;
    supportsDescendants?: boolean;
    descendantSessionCount?: number;
    warnings?: string[];
    sourceDimensions?: AgentUsageSourceDimension[];
  },
): AgentSessionUsageSummary =>
  fixtureSessionSummary({
    appType,
    sessionId,
    rootResolved: false,
    root: fixtureNode(appType, sessionId, options.title, options.projectDir),
    supportsDescendants: options.supportsDescendants ?? false,
    selfUsage: measure,
    descendantUsageStatus:
      options.descendantSessionCount && options.descendantSessionCount > 0
        ? "unavailable"
        : "not_applicable",
    descendantSessionCount: options.descendantSessionCount ?? 0,
    partial: measure?.partial ?? true,
    warnings: options.warnings ?? measure?.warnings ?? [],
    sourceDimensions:
      options.sourceDimensions ??
      (measure
        ? [fixtureDimension(appType, measure.dataSource ?? "fixture")]
        : []),
  });

const createDefaultAgentUsageFixtures = (): AgentUsageFixtureState => {
  // Explicit numeric zero is intentionally distinct from the nullable and
  // unavailable cases below.
  const claudeZero = fixtureMeasure("session_log", {
    requestCount: 0,
    inputTokens: 0,
    outputTokens: 0,
    cacheReadTokens: 0,
    cacheCreationTokens: 0,
    totalCostUsd: "0",
  });
  const codexAgentCallPartial = fixtureMeasure("codex_session", {
    inputTokens: 8,
    outputTokens: 3,
    cacheCreationTokens: null,
    totalCostUsd: null,
    requestCountSemantics: "agent_call",
    partial: true,
  });
  const hermesWindowPartial = fixtureMeasure("hermes_session_model_usage", {
    requestCount: null,
    inputTokens: 15,
    outputTokens: 5,
    cacheCreationTokens: null,
    totalCostUsd: null,
    precision: "sync_window_delta",
    timeSemantics: "sync_window_end",
    requestCountSemantics: "unavailable",
    partial: true,
  });

  const summaries = [
    fixtureSummary("claude", "claude-usage-zero", claudeZero, {
      title: "Claude zero fixture",
      projectDir: "/mock/claude-zero",
      supportsDescendants: true,
    }),
    fixtureSummary("codex", "codex-usage-agent-call", codexAgentCallPartial, {
      title: "Codex agent-call fixture",
      projectDir: "/mock/codex-agent-call",
      supportsDescendants: true,
    }),
    fixtureSummary("openclaw", "openclaw-usage-unavailable", null, {
      title: "OpenClaw unavailable fixture",
      projectDir: "/mock/openclaw-unavailable",
      warnings: ["OpenClaw usage is unavailable in this fixture."],
    }),
    fixtureSummary("hermes", "hermes-usage-window", hermesWindowPartial, {
      title: "Hermes sync-window fixture",
      projectDir: "/mock/hermes-window",
      sourceDimensions: [
        {
          ...fixtureDimension("hermes", "hermes_session_model_usage"),
          apiCallCount: 2,
          reasoningTokens: 3,
          costStatus: "unknown",
          costSource: "fixture",
          correctionState: "none",
        },
      ],
    }),
  ];

  const summaryByKey = Object.fromEntries(
    summaries.map((summary) => [
      agentUsageFixtureKey(summary.appType, summary.sessionId),
      summary,
    ]),
  ) as Record<string, AgentSessionUsageSummary>;

  const tasks: AgentTaskUsageRow[] = summaries;

  const capabilities: AgentUsageCapability[] = [
    fixtureCapability("claude", {
      supportsDescendants: true,
    }),
    fixtureCapability("claude-desktop", {
      sessionEnumeration: "partial",
      usageStatus: "partial",
      supportsDescendants: true,
      tokenStatus: "partial",
      costStatus: "partial",
    }),
    fixtureCapability("codex", {
      supportsDescendants: true,
      requestCountSemantics: "agent_call",
      tokenStatus: "partial",
      costStatus: "partial",
    }),
    fixtureCapability("gemini", {
      tokenStatus: "partial",
      costStatus: "partial",
    }),
    fixtureCapability("grokbuild", {
      requestCountSemantics: "agent_call",
      tokenStatus: "partial",
      costStatus: "partial",
    }),
    fixtureCapability("opencode"),
    fixtureCapability("openclaw", {
      sessionEnumeration: "partial",
      usageStatus: "unavailable",
      tokenStatus: "unavailable",
      costStatus: "unavailable",
      precision: "unavailable",
      timeSemantics: "unavailable",
      requestCountSemantics: "unavailable",
    }),
    fixtureCapability("hermes", {
      sessionEnumeration: "partial",
      usageStatus: "partial",
      tokenStatus: "partial",
      costStatus: "partial",
      precision: "sync_window_delta",
      timeSemantics: "sync_window_end",
      requestCountSemantics: "unavailable",
    }),
    fixtureCapability("pi", {
      supportsDescendants: true,
      costStatus: "partial",
      requestCountSemantics: "usage_event",
    }),
  ];

  return { capabilities, summaries: summaryByKey, tasks };
};

let agentUsageFixtures = createDefaultAgentUsageFixtures();

const cloneProviders = (value: ProvidersByApp) =>
  deepClone(value) as ProvidersByApp;

export const resetProviderState = () => {
  providers = createDefaultProviders();
  current = createDefaultCurrent();
  liveProviderIds = {
    opencode: [],
    openclaw: [],
    hermes: [],
  };
  sessionsState = createDefaultSessions();
  sessionMessagesState = createDefaultSessionMessages();
  agentUsageFixtures = createDefaultAgentUsageFixtures();
  settingsState = {
    showInTray: true,
    minimizeToTrayOnClose: true,
    enableClaudePluginIntegration: false,
    claudeConfigDir: "/default/claude",
    codexConfigDir: "/default/codex",
    language: "zh",
  };
  appConfigDirOverride = null;
  mcpConfigs = {
    claude: {
      sample: {
        id: "sample",
        name: "Sample Claude Server",
        enabled: true,
        apps: {
          claude: true,
          codex: false,
          gemini: false,
          opencode: false,
          openclaw: false,
          hermes: false,
        },
        server: {
          type: "stdio",
          command: "claude-server",
        },
      },
    },
    "claude-desktop": {},
    codex: {
      httpServer: {
        id: "httpServer",
        name: "HTTP Codex Server",
        enabled: false,
        apps: {
          claude: false,
          codex: true,
          gemini: false,
          opencode: false,
          openclaw: false,
          hermes: false,
        },
        server: {
          type: "http",
          url: "http://localhost:3000",
        },
      },
    },
    gemini: {},
    grokbuild: {},
    opencode: {},
    openclaw: {},
    hermes: {},
    pi: {},
  };
};

export const getProviders = (appType: AppId) =>
  cloneProviders(providers)[appType] ?? {};

export const getCurrentProviderId = (appType: AppId) => current[appType] ?? "";

export const getLiveProviderIds = (
  appType: "opencode" | "openclaw" | "hermes",
) => [...liveProviderIds[appType]];

export const setLiveProviderIds = (
  appType: "opencode" | "openclaw" | "hermes",
  ids: string[],
) => {
  liveProviderIds[appType] = [...ids];
};

export const setCurrentProviderId = (appType: AppId, providerId: string) => {
  current[appType] = providerId;
};

export const updateProviders = (
  appType: AppId,
  data: Record<string, Provider>,
) => {
  providers[appType] = cloneProviders({ [appType]: data } as ProvidersByApp)[
    appType
  ];
};

export const setProviders = (
  appType: AppId,
  data: Record<string, Provider>,
) => {
  providers[appType] = deepClone(data) as Record<string, Provider>;
};

export const addProvider = (appType: AppId, provider: Provider) => {
  providers[appType] = providers[appType] ?? {};
  providers[appType][provider.id] = provider;
};

export const updateProvider = (appType: AppId, provider: Provider) => {
  if (!providers[appType]) return;
  providers[appType][provider.id] = {
    ...providers[appType][provider.id],
    ...provider,
  };
};

export const deleteProvider = (appType: AppId, providerId: string) => {
  if (!providers[appType]) return;
  delete providers[appType][providerId];
  if (current[appType] === providerId) {
    const fallback = Object.keys(providers[appType])[0] ?? "";
    current[appType] = fallback;
  }
};

export const updateSortOrder = (
  appType: AppId,
  updates: { id: string; sortIndex: number }[],
) => {
  if (!providers[appType]) return;
  updates.forEach(({ id, sortIndex }) => {
    const provider = providers[appType][id];
    if (provider) {
      providers[appType][id] = { ...provider, sortIndex };
    }
  });
};

export const listProviders = (appType: AppId) =>
  deepClone(providers[appType] ?? {}) as Record<string, Provider>;

export const getSettings = () => deepClone(settingsState) as Settings;

export const setSettings = (data: Partial<Settings>) => {
  settingsState = { ...settingsState, ...data };
};

export const getAppConfigDirOverride = () => appConfigDirOverride;

export const setAppConfigDirOverrideState = (value: string | null) => {
  appConfigDirOverride = value;
};

export const getMcpConfig = (appType: AppId) => {
  const servers = deepClone(mcpConfigs[appType] ?? {}) as Record<
    string,
    McpServer
  >;
  return {
    configPath: `/mock/${appType}.mcp.json`,
    servers,
  };
};

export const setMcpConfig = (
  appType: AppId,
  value: Record<string, McpServer>,
) => {
  mcpConfigs[appType] = deepClone(value) as Record<string, McpServer>;
};

export const setMcpServerEnabled = (
  appType: AppId,
  id: string,
  enabled: boolean,
) => {
  if (!mcpConfigs[appType]?.[id]) return;
  mcpConfigs[appType][id] = {
    ...mcpConfigs[appType][id],
    enabled,
  };
};

export const upsertMcpServer = (
  appType: AppId,
  id: string,
  server: McpServer,
) => {
  if (!mcpConfigs[appType]) {
    mcpConfigs[appType] = {};
  }
  mcpConfigs[appType][id] = deepClone(server) as McpServer;
};

export const deleteMcpServer = (appType: AppId, id: string) => {
  if (!mcpConfigs[appType]) return;
  delete mcpConfigs[appType][id];
};

export const listSessions = () => deepClone(sessionsState) as SessionMeta[];

export const getSessionMessages = (providerId: string, sourcePath: string) =>
  deepClone(
    sessionMessagesState[sessionMessageKey(providerId, sourcePath)] ?? [],
  ) as SessionMessage[];

export const deleteSession = (
  providerId: string,
  sessionId: string,
  sourcePath: string,
) => {
  sessionsState = sessionsState.filter(
    (session) =>
      !(
        session.providerId === providerId &&
        session.sessionId === sessionId &&
        session.sourcePath === sourcePath
      ),
  );
  delete sessionMessagesState[sessionMessageKey(providerId, sourcePath)];
  return true;
};

export const setSessionFixtures = (
  sessions: SessionMeta[],
  messages: Record<string, SessionMessage[]>,
) => {
  sessionsState = deepClone(sessions) as SessionMeta[];
  sessionMessagesState = deepClone(messages) as Record<
    string,
    SessionMessage[]
  >;
};

export const getAgentSessionUsageFixture = (
  request: AgentSessionUsageRequest,
): AgentSessionUsageSummary => {
  const key = agentUsageFixtureKey(request.appType, request.sessionId);
  const existing = agentUsageFixtures.summaries[key];
  if (existing) return deepClone(existing);

  const fallbackNode = fixtureNode(
    request.appType,
    request.sessionId,
    "Unknown usage fixture",
    "/mock/unknown",
  );
  return {
    appType: request.appType,
    requestedSessionId: request.sessionId,
    sessionId: request.sessionId,
    rootSessionId: request.sessionId,
    rootResolved: false,
    root: fallbackNode,
    supportsDescendants: false,
    selfUsage: null,
    descendantUsage: null,
    descendantUsageStatus: "not_applicable",
    totalUsage: null,
    descendantSessionCount: 0,
    precision: "unavailable",
    partial: true,
    warnings: ["No anonymous usage fixture exists for this session."],
    sourceDimensions: [],
  };
};

export const getAgentTaskUsageFixture = (
  filter: AgentTaskUsageFilter = {},
): AgentTaskUsagePage => {
  const titleNeedle = filter.title?.toLowerCase();
  const projectNeedle = filter.project?.toLowerCase();
  const titleExact = filter.titleExact?.trim().toLowerCase();
  const projectDirExact = filter.projectDirExact?.trim().toLowerCase();
  const filtered = agentUsageFixtures.tasks.filter((task) => {
    if (filter.appType && task.appType !== filter.appType) return false;
    const title = task.root?.title?.toLowerCase() ?? "";
    const projectDir = task.root?.projectDir?.toLowerCase() ?? "";
    if (titleExact && title !== titleExact) return false;
    if (projectDirExact && projectDir !== projectDirExact) return false;
    if (titleNeedle && !title.includes(titleNeedle)) return false;
    if (projectNeedle && !projectDir.includes(projectNeedle)) return false;
    if (
      filter.projectDir !== undefined &&
      task.root?.projectDir !== filter.projectDir
    ) {
      return false;
    }
    return true;
  });
  const limit = Math.max(0, Math.min(filter.limit ?? 50, 500));
  const offset = Math.max(0, filter.offset ?? 0);
  return {
    items: deepClone(filtered.slice(offset, offset + limit)),
    total: filtered.length,
    limit,
    offset,
    hasMore: offset + limit < filtered.length,
    unattributedUsage: null,
  };
};

export const getAgentTaskUsageFilterOptionsFixture = (
  request: AgentTaskUsageFilterOptionsRequest = {},
): AgentTaskUsageFilterOptions => {
  const tasks = agentUsageFixtures.tasks.filter(
    (task) => !request.appType || task.appType === request.appType,
  );
  const titles = new Map<string, string>();
  const projects = new Map<string, string>();
  for (const task of tasks) {
    const title = task.root?.title?.trim();
    if (title) titles.set(title.toLowerCase(), title);
    const projectDir = task.root?.projectDir?.trim();
    if (projectDir) projects.set(projectDir.toLowerCase(), projectDir);
  }
  return {
    titles: Array.from(titles.values()).sort((left, right) =>
      left.localeCompare(right),
    ),
    projects: Array.from(projects.values())
      .sort((left, right) => left.localeCompare(right))
      .map((projectDir) => ({ projectDir })),
  };
};

export const getAgentUsageCapabilitiesFixture = (): AgentUsageCapability[] =>
  deepClone(agentUsageFixtures.capabilities);

export const setAgentUsageFixtures = (
  fixtures: Partial<AgentUsageFixtureState>,
) => {
  agentUsageFixtures = {
    ...agentUsageFixtures,
    ...deepClone(fixtures),
  };
};
