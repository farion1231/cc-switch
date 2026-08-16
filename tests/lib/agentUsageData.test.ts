import { describe, expect, it } from "vitest";
import { usageApi } from "@/lib/api/usage";
import {
  normalizeAgentTaskUsageFilter,
  normalizeAgentUsageRange,
  usageKeys,
} from "@/lib/query/usage";

describe("canonical Agent usage data contract", () => {
  it("keeps zero, null/partial, unavailable, and request-count semantics intact", async () => {
    const zero = await usageApi.getAgentSessionUsage({
      appType: "claude",
      sessionId: "claude-usage-zero",
    });
    expect(zero.selfUsage?.inputTokens).toBe(0);
    expect(zero.selfUsage?.cacheCreationTokens).toBe(0);
    expect(zero.selfUsage?.totalCostUsd).toBe("0");
    expect(zero.selfUsage?.partial).toBe(false);
    expect(zero.selfUsage?.requestCountSemantics).toBe("assistant_message");

    const codex = await usageApi.getAgentSessionUsage({
      appType: "codex",
      sessionId: "codex-usage-agent-call",
    });
    expect(codex.selfUsage?.cacheCreationTokens).toBeNull();
    expect(codex.selfUsage?.totalCostUsd).toBeNull();
    expect(codex.selfUsage?.partial).toBe(true);
    expect(codex.selfUsage?.requestCountSemantics).toBe("agent_call");

    const unavailable = await usageApi.getAgentSessionUsage({
      appType: "openclaw",
      sessionId: "openclaw-usage-unavailable",
    });
    expect(unavailable.totalUsage).toBeNull();
    expect(unavailable.precision).toBe("unavailable");
    expect(unavailable.partial).toBe(true);

    const capabilities = await usageApi.getAgentUsageCapabilities();
    expect(capabilities).toHaveLength(9);
    expect(capabilities.map(({ appType }) => appType)).toEqual([
      "claude",
      "claude-desktop",
      "codex",
      "gemini",
      "grokbuild",
      "opencode",
      "openclaw",
      "hermes",
      "pi",
    ]);
  });

  it("isolates session/range and task filter pagination query keys", () => {
    const noRange = normalizeAgentUsageRange();
    const week = normalizeAgentUsageRange({ startAt: 100, endAt: 200 });
    const month = normalizeAgentUsageRange({ startAt: 100, endAt: 300 });
    const taskFilter = (
      overrides: Parameters<typeof normalizeAgentTaskUsageFilter>[0] = {},
    ) =>
      normalizeAgentTaskUsageFilter({
        appType: "claude",
        title: "first",
        project: "project-a",
        projectDir: "/mock/a",
        range: { startAt: 100, endAt: 200 },
        limit: 10,
        offset: 0,
        ...overrides,
      });

    expect(usageKeys.agentSession("claude", "same", noRange)).not.toEqual(
      usageKeys.agentSession("codex", "same", noRange),
    );
    expect(usageKeys.agentSession("claude", "same", noRange)).not.toEqual(
      usageKeys.agentSession("claude", "other", noRange),
    );
    expect(usageKeys.agentSession("claude", "same", week)).not.toEqual(
      usageKeys.agentSession("claude", "same", month),
    );

    const firstPage = taskFilter();
    const secondPage = taskFilter({ offset: 10 });
    expect(usageKeys.agentTasks(firstPage)).not.toEqual(
      usageKeys.agentTasks(secondPage),
    );

    const changedFilter = taskFilter({ projectDir: "/mock/b" });
    expect(usageKeys.agentTasks(firstPage)).not.toEqual(
      usageKeys.agentTasks(changedFilter),
    );

    const exactTitle = taskFilter({
      title: undefined,
      project: undefined,
      projectDir: undefined,
      titleExact: "Build",
      projectDirExact: "/mock/a",
    });
    expect(usageKeys.agentTasks(firstPage)).not.toEqual(
      usageKeys.agentTasks(exactTitle),
    );

    expect(usageKeys.agentTaskFilterOptions("claude", week)).not.toEqual(
      usageKeys.agentTaskFilterOptions("codex", week),
    );
    expect(usageKeys.agentTaskFilterOptions("claude", week)).not.toEqual(
      usageKeys.agentTaskFilterOptions("claude", month),
    );
  });
});
