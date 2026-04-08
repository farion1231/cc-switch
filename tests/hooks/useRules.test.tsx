import type { ReactNode } from "react";
import { act, renderHook } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  useInstallRule,
  useInstallRulesFromZip,
  type DiscoverableRule,
  type InstalledRule,
} from "@/hooks/useRules";

const installUnifiedMock = vi.fn();
const installFromZipMock = vi.fn();

vi.mock("@/lib/api/rules", () => ({
  rulesApi: {
    installUnified: (...args: unknown[]) => installUnifiedMock(...args),
    installFromZip: (...args: unknown[]) => installFromZipMock(...args),
  },
}));

interface WrapperProps {
  children: ReactNode;
}

function createWrapper() {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  });

  const wrapper = ({ children }: WrapperProps) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );

  return { wrapper, queryClient };
}

function createInstalledRule(
  overrides: Partial<InstalledRule> = {},
): InstalledRule {
  return {
    id: "owner/repo/rule-a",
    name: "Rule A",
    description: "Rule A description",
    directory: "rule-a",
    repoOwner: "owner",
    repoName: "repo",
    repoBranch: "main",
    readmeUrl: "https://example.com/readme",
    apps: {
      claude: true,
      codex: false,
      gemini: false,
      opencode: false,
      openclaw: false,
    },
    installedAt: 1,
    ...overrides,
  };
}

function createDiscoverableRule(
  overrides: Partial<DiscoverableRule> = {},
): DiscoverableRule {
  return {
    key: "owner/repo/rule-a",
    name: "Rule A",
    description: "Rule A description",
    directory: "rule-a",
    readmeUrl: "https://example.com/readme",
    repoOwner: "owner",
    repoName: "repo",
    repoBranch: "main",
    ...overrides,
  };
}

beforeEach(() => {
  installUnifiedMock.mockReset();
  installFromZipMock.mockReset();
});

describe("useRules cache updates", () => {
  it("upserts installed rules by id when install returns an existing row", async () => {
    const { wrapper, queryClient } = createWrapper();
    const existingRule = createInstalledRule();
    const updatedRule = createInstalledRule({
      apps: {
        claude: true,
        codex: true,
        gemini: false,
        opencode: false,
        openclaw: false,
      },
      installedAt: 2,
    });
    const discoverableRule = createDiscoverableRule();

    queryClient.setQueryData<InstalledRule[]>(["rules", "installed"], [
      existingRule,
    ]);
    queryClient.setQueryData<DiscoverableRule[]>(["rules", "discoverable"], [
      discoverableRule,
    ]);
    installUnifiedMock.mockResolvedValueOnce(updatedRule);

    const { result } = renderHook(() => useInstallRule(), { wrapper });

    await act(async () => {
      await result.current.mutateAsync({
        rule: discoverableRule,
        currentApp: "codex",
      });
    });

    expect(installUnifiedMock).toHaveBeenCalledWith(discoverableRule, "codex");
    expect(queryClient.getQueryData(["rules", "installed"])).toEqual([
      updatedRule,
    ]);
    expect(queryClient.getQueryData(["rules", "discoverable"])).toEqual([
      { ...discoverableRule, installed: true },
    ]);
  });

  it("deduplicates zip install results against existing installed rows", async () => {
    const { wrapper, queryClient } = createWrapper();
    const existingRule = createInstalledRule();
    const updatedRule = createInstalledRule({
      apps: {
        claude: true,
        codex: true,
        gemini: false,
        opencode: false,
        openclaw: false,
      },
    });
    const newRule = createInstalledRule({
      id: "owner/repo/rule-b",
      name: "Rule B",
      directory: "rule-b",
    });

    queryClient.setQueryData<InstalledRule[]>(["rules", "installed"], [
      existingRule,
    ]);
    installFromZipMock.mockResolvedValueOnce([updatedRule, newRule]);

    const { result } = renderHook(() => useInstallRulesFromZip(), { wrapper });

    await act(async () => {
      await result.current.mutateAsync({
        filePath: "/tmp/rules.zip",
        currentApp: "codex",
      });
    });

    expect(installFromZipMock).toHaveBeenCalledWith("/tmp/rules.zip", "codex");
    expect(queryClient.getQueryData(["rules", "installed"])).toEqual([
      updatedRule,
      newRule,
    ]);
  });

  it("deduplicates duplicate ids within a fresh zip install result", async () => {
    const { wrapper, queryClient } = createWrapper();
    const firstVersion = createInstalledRule({ installedAt: 1 });
    const latestVersion = createInstalledRule({
      installedAt: 2,
      apps: {
        claude: true,
        codex: true,
        gemini: false,
        opencode: false,
        openclaw: false,
      },
    });

    installFromZipMock.mockResolvedValueOnce([firstVersion, latestVersion]);

    const { result } = renderHook(() => useInstallRulesFromZip(), { wrapper });

    await act(async () => {
      await result.current.mutateAsync({
        filePath: "/tmp/rules.zip",
        currentApp: "codex",
      });
    });

    expect(queryClient.getQueryData(["rules", "installed"])).toEqual([
      latestVersion,
    ]);
  });
});
