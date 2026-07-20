import type { ReactNode } from "react";
import { renderHook, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";

import { useDiscoverableSkills } from "@/hooks/useSkills";
import type { DiscoverableSkill } from "@/lib/api/skills";

const api = vi.hoisted(() => ({
  loadCachedDiscoverable: vi.fn(),
  discoverAvailable: vi.fn(),
  getRepos: vi.fn(),
}));

vi.mock("@/lib/api/skills", () => ({ skillsApi: api }));

const skill = (name: string): DiscoverableSkill => ({
  key: `owner/repo:${name}`,
  name,
  description: name,
  directory: name,
  repoOwner: "owner",
  repoName: "repo",
  repoBranch: "main",
});

describe("useDiscoverableSkills", () => {
  it("shows persisted results while the remote refresh is pending", async () => {
    let resolveRemote!: (skills: DiscoverableSkill[]) => void;
    api.loadCachedDiscoverable.mockResolvedValue([skill("cached")]);
    api.discoverAvailable.mockReturnValue(
      new Promise<DiscoverableSkill[]>((resolve) => {
        resolveRemote = resolve;
      }),
    );
    api.getRepos.mockResolvedValue([
      { owner: "owner", name: "repo", branch: "main", enabled: true },
    ]);

    const client = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const wrapper = ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={client}>{children}</QueryClientProvider>
    );
    const { result } = renderHook(() => useDiscoverableSkills(), { wrapper });

    await waitFor(() => expect(result.current.data?.[0].name).toBe("cached"));
    resolveRemote([skill("fresh")]);
    await waitFor(() => expect(result.current.data?.[0].name).toBe("fresh"));
  });
});
