import type { ReactNode } from "react";
import { act, renderHook } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  useInstallAgent,
  useInstallAgentsFromZip,
  type DiscoverableAgent,
  type InstalledAgent,
} from "@/hooks/useAgents";

const installUnifiedMock = vi.fn();
const installFromZipMock = vi.fn();

vi.mock("@/lib/api/agents", () => ({
  agentsApi: {
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

function createInstalledAgent(
  overrides: Partial<InstalledAgent> = {},
): InstalledAgent {
  return {
    id: "owner/repo/agent-a",
    name: "Agent A",
    description: "Agent A description",
    directory: "agent-a.md",
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

function createDiscoverableAgent(
  overrides: Partial<DiscoverableAgent> = {},
): DiscoverableAgent {
  return {
    key: "owner/repo/agent-a",
    name: "Agent A",
    description: "Agent A description",
    directory: "agents/agent-a.md",
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

describe("useAgents cache updates", () => {
  it("upserts installed agents by id when install returns an existing row", async () => {
    const { wrapper, queryClient } = createWrapper();
    const existingAgent = createInstalledAgent();
    const updatedAgent = createInstalledAgent({
      apps: {
        claude: true,
        codex: true,
        gemini: false,
        opencode: false,
        openclaw: false,
      },
      installedAt: 2,
    });
    const discoverableAgent = createDiscoverableAgent();

    queryClient.setQueryData<InstalledAgent[]>(["agents", "installed"], [
      existingAgent,
    ]);
    queryClient.setQueryData<DiscoverableAgent[]>(["agents", "discoverable"], [
      discoverableAgent,
    ]);
    installUnifiedMock.mockResolvedValueOnce(updatedAgent);

    const { result } = renderHook(() => useInstallAgent(), { wrapper });

    await act(async () => {
      await result.current.mutateAsync({
        agent: discoverableAgent,
        currentApp: "codex",
      });
    });

    expect(installUnifiedMock).toHaveBeenCalledWith(discoverableAgent, "codex");
    expect(queryClient.getQueryData(["agents", "installed"])).toEqual([
      updatedAgent,
    ]);
    expect(queryClient.getQueryData(["agents", "discoverable"])).toEqual([
      { ...discoverableAgent, installed: true },
    ]);
  });

  it("deduplicates zip install results against existing installed rows", async () => {
    const { wrapper, queryClient } = createWrapper();
    const existingAgent = createInstalledAgent();
    const updatedAgent = createInstalledAgent({
      apps: {
        claude: true,
        codex: true,
        gemini: false,
        opencode: false,
        openclaw: false,
      },
    });
    const newAgent = createInstalledAgent({
      id: "owner/repo/agent-b",
      name: "Agent B",
      directory: "agent-b.md",
    });

    queryClient.setQueryData<InstalledAgent[]>(["agents", "installed"], [
      existingAgent,
    ]);
    installFromZipMock.mockResolvedValueOnce([updatedAgent, newAgent]);

    const { result } = renderHook(() => useInstallAgentsFromZip(), { wrapper });

    await act(async () => {
      await result.current.mutateAsync({
        filePath: "/tmp/agents.zip",
        currentApp: "codex",
      });
    });

    expect(installFromZipMock).toHaveBeenCalledWith("/tmp/agents.zip", "codex");
    expect(queryClient.getQueryData(["agents", "installed"])).toEqual([
      updatedAgent,
      newAgent,
    ]);
  });

  it("deduplicates duplicate ids within a fresh zip install result", async () => {
    const { wrapper, queryClient } = createWrapper();
    const firstVersion = createInstalledAgent({ installedAt: 1 });
    const latestVersion = createInstalledAgent({
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

    const { result } = renderHook(() => useInstallAgentsFromZip(), { wrapper });

    await act(async () => {
      await result.current.mutateAsync({
        filePath: "/tmp/agents.zip",
        currentApp: "codex",
      });
    });

    expect(queryClient.getQueryData(["agents", "installed"])).toEqual([
      latestVersion,
    ]);
  });
});
