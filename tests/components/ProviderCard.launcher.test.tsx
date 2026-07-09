import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { Provider } from "@/types";
import type { ClaudeShortcutCommandResult } from "@/lib/api/providers";
import { ProviderCard } from "@/components/providers/ProviderCard";

const getClaudeShortcutStatusMock = vi.hoisted(() => vi.fn());

vi.mock("@/lib/api/providers", () => ({
  providersApi: {
    getClaudeShortcutStatus: getClaudeShortcutStatusMock,
  },
}));

vi.mock("@/lib/query/failover", () => ({
  useProviderHealth: () => ({ data: null }),
}));

vi.mock("@/lib/query/queries", () => ({
  useUsageQuery: () => ({ data: null }),
}));

vi.mock("@/components/providers/ProviderActions", () => ({
  ProviderActions: () => <div data-testid="provider-actions" />,
}));

vi.mock("@/components/UsageFooter", () => ({
  default: () => <div data-testid="usage-footer" />,
}));

vi.mock("@/components/SubscriptionQuotaFooter", () => ({
  default: () => <div data-testid="subscription-footer" />,
}));

vi.mock("@/components/CopilotQuotaFooter", () => ({
  default: () => <div data-testid="copilot-footer" />,
}));

vi.mock("@/components/CodexOauthQuotaFooter", () => ({
  default: () => <div data-testid="codex-oauth-footer" />,
}));

const baseProvider: Provider = {
  id: "kimi",
  name: "Kimi",
  category: "third_party",
  settingsConfig: {
    env: {
      ANTHROPIC_BASE_URL: "https://api.example.com",
    },
  },
  meta: {
    parallelConfigEnabled: true,
    shortcutName: "claude-kimi",
  },
};

function launcherResult(
  status: ClaudeShortcutCommandResult["info"]["status"],
): ClaudeShortcutCommandResult {
  return {
    info: {
      name: "claude-kimi",
      targetPath: "/Users/test/.local/bin/claude-kimi",
      status,
      currentProfileDir: null,
    },
    targetKind: "user",
    userBinDir: "/Users/test/.local/bin",
    pathOnPath: true,
    pathExportSnippet: null,
    launchCommand: null,
    installed: status === "installed",
    removed: false,
    error: null,
  };
}

function renderCard(provider: Provider = baseProvider) {
  return render(
    <ProviderCard
      provider={provider}
      isCurrent={false}
      appId="claude"
      onSwitch={vi.fn()}
      onEdit={vi.fn()}
      onDelete={vi.fn()}
      onConfigureUsage={vi.fn()}
      onOpenWebsite={vi.fn()}
      onDuplicate={vi.fn()}
      isProxyRunning={false}
    />,
  );
}

describe("ProviderCard launcher status", () => {
  it("shows launcher alias and permission mode when installed", async () => {
    getClaudeShortcutStatusMock.mockResolvedValueOnce(
      launcherResult("installed"),
    );

    renderCard({
      ...baseProvider,
      meta: {
        ...baseProvider.meta,
        launcherPermissionMode: "acceptEdits",
      },
    });

    expect(
      await screen.findByText("Launcher: claude-kimi · acceptEdits"),
    ).toBeInTheDocument();
  });

  it.each([
    ["missing", "Launcher: claude-kimi · command missing"],
    ["stale", "Launcher: claude-kimi · needs update"],
    ["conflict", "Launcher: claude-kimi · conflict"],
  ] as const)("shows compact %s launcher health", async (status, label) => {
    getClaudeShortcutStatusMock.mockResolvedValueOnce(launcherResult(status));

    renderCard();

    expect(await screen.findByText(label)).toBeInTheDocument();
  });
});
