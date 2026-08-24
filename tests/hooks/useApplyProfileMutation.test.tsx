import type { ReactNode } from "react";
import { act, renderHook } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useApplyProfileMutation } from "@/lib/query/profiles";

const apiMocks = vi.hoisted(() => ({
  apply: vi.fn(),
  updateTrayMenu: vi.fn(),
}));

const toastMocks = vi.hoisted(() => ({
  success: vi.fn(),
  error: vi.fn(),
  warning: vi.fn(),
}));

vi.mock("@/lib/api", () => ({
  profilesApi: {
    apply: (...args: unknown[]) => apiMocks.apply(...args),
  },
  providersApi: {
    updateTrayMenu: (...args: unknown[]) => apiMocks.updateTrayMenu(...args),
  },
}));

vi.mock("sonner", () => ({ toast: toastMocks }));

function createWrapper() {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  });
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );

  return { queryClient, wrapper };
}

describe("useApplyProfileMutation", () => {
  beforeEach(() => {
    apiMocks.apply.mockReset().mockResolvedValue([]);
    apiMocks.updateTrayMenu.mockReset().mockResolvedValue(undefined);
    toastMocks.success.mockReset();
    toastMocks.error.mockReset();
    toastMocks.warning.mockReset();
  });

  it("invalidates the independent Codex Desktop provider cache", async () => {
    const { queryClient, wrapper } = createWrapper();
    const invalidateSpy = vi.spyOn(queryClient, "invalidateQueries");
    const { result } = renderHook(() => useApplyProfileMutation(), { wrapper });

    await act(async () => {
      await result.current.mutateAsync({
        id: "desktop-profile",
        scope: "codex-desktop",
      });
    });

    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: ["providers", "codex-desktop"],
    });
    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: ["codexDesktopStatus"],
    });
  });
});
