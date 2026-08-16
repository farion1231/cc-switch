import type {
  AgentSessionNodeView,
  AgentSessionUsageSummary,
  AgentTaskUsageRow,
  AgentUsageCapability,
  AgentUsageMeasure,
  AgentUsageSourceDimension,
} from "@/types/usage";

export const createAgentUsageMeasure = (
  overrides: Partial<AgentUsageMeasure> = {},
): AgentUsageMeasure => ({
  dataSource: "fixture",
  requestCount: 1,
  inputTokens: 10,
  outputTokens: 5,
  cacheReadTokens: 0,
  cacheCreationTokens: 0,
  totalCostUsd: "0.01",
  precision: "request_exact",
  timeSemantics: "event_time",
  requestCountSemantics: "assistant_message",
  partial: false,
  warnings: [],
  ...overrides,
});

export const createAgentSessionNode = (
  appType: AgentSessionNodeView["appType"],
  sessionId: string,
  overrides: Partial<AgentSessionNodeView> = {},
): AgentSessionNodeView => ({
  appType,
  sessionId,
  parentSessionId: null,
  rootSessionId: sessionId,
  nodeKind: "standalone",
  relationConfidence: "unavailable",
  title: "Fixture task",
  projectDir: "/mock/project",
  sourcePath: `/mock/${appType}/${sessionId}.jsonl`,
  createdAt: 1_723_000_000,
  lastActiveAt: 1_723_000_100,
  lastSyncedAt: 1_723_000_100,
  ...overrides,
});

export const createAgentUsageSourceDimension = (
  appType: AgentSessionNodeView["appType"] = "codex",
  dataSource = "codex_session",
  overrides: Partial<AgentUsageSourceDimension> = {},
): AgentUsageSourceDimension => ({
  providerId: `${appType}-provider`,
  model: `${appType}-model`,
  requestModel: `${appType}-request-model`,
  pricingModel: `${appType}-pricing-model`,
  dataSource,
  inputTokenSemantics: 1,
  sourceIdentity: `${appType}-fixture-source`,
  profileId: "fixture-profile",
  databaseIdentity: "fixture-database",
  baseUrlDigest: "fixture-base-url",
  billingMode: "fixture",
  task: "fixture-task",
  sourceVersion: "fixture-v1",
  syncWindowStart: 1_723_000_000,
  syncWindowEnd: 1_723_000_100,
  apiCallCount: null,
  cacheWriteTokens: null,
  reasoningTokens: null,
  costStatus: null,
  costSource: null,
  costDeltaKind: null,
  correctionState: null,
  rangePartial: false,
  ...overrides,
});

export const createAgentUsageCapability = (
  appType: AgentUsageCapability["appType"],
  overrides: Partial<AgentUsageCapability> = {},
): AgentUsageCapability => ({
  appType,
  sessionEnumeration: "supported",
  usageStatus: "supported",
  supportsDescendants: false,
  tokenStatus: "supported",
  costStatus: "supported",
  precision: "request_exact",
  timeSemantics: "event_time",
  requestCountSemantics: "assistant_message",
  notes: "Anonymous fixture",
  ...overrides,
});

const createUsageProjection = (overrides: Partial<AgentTaskUsageRow>) => {
  const selfUsage =
    overrides.selfUsage === undefined
      ? createAgentUsageMeasure()
      : overrides.selfUsage;
  return {
    selfUsage,
    descendantUsage: overrides.descendantUsage ?? null,
    descendantUsageStatus: overrides.descendantUsageStatus ?? "not_applicable",
    totalUsage:
      overrides.totalUsage === undefined ? selfUsage : overrides.totalUsage,
    descendantSessionCount: overrides.descendantSessionCount ?? 0,
    precision: overrides.precision ?? selfUsage?.precision ?? "unavailable",
    partial: overrides.partial ?? selfUsage?.partial ?? true,
    warnings: overrides.warnings ?? selfUsage?.warnings ?? [],
    sourceDimensions: overrides.sourceDimensions ?? [],
  };
};

export const createAgentTaskUsageRow = (
  overrides: Partial<AgentTaskUsageRow> = {},
): AgentTaskUsageRow => {
  const appType = overrides.appType ?? "claude";
  const sessionId = overrides.sessionId ?? "root-session";
  const rootSessionId = overrides.rootSessionId ?? sessionId;
  const root = createAgentSessionNode(appType, sessionId, {
    nodeKind: "root",
    relationConfidence: "explicit",
    title: "Root task",
    projectDir: "/workspace/project",
    sourcePath: null,
    createdAt: 100,
    lastActiveAt: 200,
    lastSyncedAt: 200,
    rootSessionId,
  });
  return {
    appType,
    sessionId,
    rootSessionId,
    root: overrides.root === undefined ? root : overrides.root,
    ...createUsageProjection(overrides),
    ...overrides,
  };
};

export const createAgentSessionUsageSummary = (
  overrides: Partial<AgentSessionUsageSummary> = {},
): AgentSessionUsageSummary => {
  const appType = overrides.appType ?? "codex";
  const sessionId = overrides.sessionId ?? "root-session";
  return {
    appType,
    requestedSessionId: overrides.requestedSessionId ?? sessionId,
    sessionId,
    rootSessionId: overrides.rootSessionId ?? sessionId,
    rootResolved: true,
    root: null,
    supportsDescendants: false,
    ...createUsageProjection(overrides),
    ...overrides,
  };
};
