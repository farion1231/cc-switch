import type { ReactNode } from "react";
import { renderHook, waitFor } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Provider } from "@/types";
import { createTestQueryClient } from "../utils/testQueryClient";

const api = vi.hoisted(() => ({
  getAll: vi.fn(),
  getCurrent: vi.fn(),
}));

vi.mock("@/lib/api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/api")>();
  return { ...actual, providersApi: api };
});

import { useAvailableProvidersForFailover } from "@/lib/query/failover";

const provider = (
  overrides: Partial<Provider> & Pick<Provider, "id" | "name">,
): Provider => ({
  settingsConfig: {},
  ...overrides,
});

const wrapper = ({ children }: { children: ReactNode }) => (
  <QueryClientProvider client={createTestQueryClient()}>
    {children}
  </QueryClientProvider>
);

beforeEach(() => {
  api.getAll.mockReset();
  api.getCurrent.mockReset();
  api.getCurrent.mockResolvedValue("");
});

describe("useAvailableProvidersForFailover", () => {
  it("lists only queue-eligible providers that are not queued yet, in home order", async () => {
    api.getAll.mockResolvedValue({
      "b-second": provider({
        id: "b-second",
        name: "Second",
        category: "third_party",
        sortIndex: 2,
      }),
      "a-first": provider({
        id: "a-first",
        name: "First",
        category: "third_party",
        sortIndex: 1,
      }),
      queued: provider({
        id: "queued",
        name: "Queued",
        category: "third_party",
        sortIndex: 3,
        inFailoverQueue: true,
      }),
      "codex-official": provider({
        id: "codex-official",
        name: "Codex Official",
        category: "official",
        sortIndex: 4,
      }),
    });

    const { result } = renderHook(
      () => useAvailableProvidersForFailover("codex"),
      { wrapper },
    );

    await waitFor(() => expect(result.current.isSuccess).toBe(true));

    expect(result.current.data?.map((item) => item.id)).toEqual([
      "a-first",
      "b-second",
    ]);
    expect(api.getAll).toHaveBeenCalledWith("codex");
  });

  it("stays idle when disabled", async () => {
    api.getAll.mockResolvedValue({});

    const { result } = renderHook(
      () => useAvailableProvidersForFailover("codex", false),
      { wrapper },
    );

    expect(result.current.data).toBeUndefined();
    expect(api.getAll).not.toHaveBeenCalled();
  });
});
