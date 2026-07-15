import type { ReactNode } from "react";
import { act, renderHook } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useSwitchProviderMutation } from "@/lib/query/mutations";

const apiMocks = vi.hoisted(() => ({
  switch: vi.fn(),
  updateTrayMenu: vi.fn(),
}));

vi.mock("@/lib/api", () => ({
  providersApi: {
    switch: (...args: unknown[]) => apiMocks.switch(...args),
    updateTrayMenu: (...args: unknown[]) => apiMocks.updateTrayMenu(...args),
  },
  sessionsApi: {},
  settingsApi: {},
}));

vi.mock("@/hooks/useHermes", () => ({
  invalidateHermesProviderCaches: vi.fn(),
}));

vi.mock("@/hooks/useOpenClaw", () => ({
  openclawKeys: {
    liveProviderIds: ["openclaw", "live-provider-ids"],
    defaultModel: ["openclaw", "default-model"],
    health: ["openclaw", "health"],
  },
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (_key: string, options?: { defaultValue?: string }) =>
      options?.defaultValue ?? _key,
  }),
}));

vi.mock("sonner", () => ({
  toast: {
    error: vi.fn(),
  },
}));

function createWrapper() {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  });
  const invalidateSpy = vi.spyOn(queryClient, "invalidateQueries");
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );

  return { wrapper, invalidateSpy };
}

beforeEach(() => {
  apiMocks.switch.mockReset();
  apiMocks.updateTrayMenu.mockReset().mockResolvedValue(true);
});

describe("useSwitchProviderMutation", () => {
  it("invalidates proxy state after a seamless switch", async () => {
    apiMocks.switch.mockResolvedValue({
      warnings: [],
      seamless: true,
      restartRequired: true,
    });
    const { wrapper, invalidateSpy } = createWrapper();
    const { result } = renderHook(() => useSwitchProviderMutation("claude"), {
      wrapper,
    });

    await act(async () => {
      await result.current.mutateAsync({
        providerId: "provider-b",
        seamless: true,
      });
    });

    expect(apiMocks.switch).toHaveBeenCalledWith("provider-b", "claude", true);
    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: ["proxyStatus"],
    });
    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: ["proxyRunning"],
    });
    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: ["proxyTakeoverStatus"],
    });
  });

  it("does not invalidate proxy state after a normal switch", async () => {
    apiMocks.switch.mockResolvedValue({
      warnings: [],
      seamless: false,
      restartRequired: false,
    });
    const { wrapper, invalidateSpy } = createWrapper();
    const { result } = renderHook(() => useSwitchProviderMutation("codex"), {
      wrapper,
    });

    await act(async () => {
      await result.current.mutateAsync({
        providerId: "codex-official",
        seamless: false,
      });
    });

    expect(invalidateSpy).not.toHaveBeenCalledWith({
      queryKey: ["proxyStatus"],
    });
    expect(invalidateSpy).not.toHaveBeenCalledWith({
      queryKey: ["proxyRunning"],
    });
    expect(invalidateSpy).not.toHaveBeenCalledWith({
      queryKey: ["proxyTakeoverStatus"],
    });
  });
});
