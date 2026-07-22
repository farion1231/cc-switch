import type { ReactNode } from "react";
import { act, renderHook } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { useSwitchProviderMutation } from "@/lib/query/mutations";

const apiMocks = vi.hoisted(() => ({
  switchProvider: vi.fn(),
  switchManagedTargetProvider: vi.fn(),
  updateTrayMenu: vi.fn(),
}));

vi.mock("@/lib/api", () => ({
  providersApi: {
    switch: (...args: unknown[]) => apiMocks.switchProvider(...args),
    updateTrayMenu: (...args: unknown[]) => apiMocks.updateTrayMenu(...args),
  },
  sessionsApi: {},
  settingsApi: {
    switchManagedTargetProvider: (...args: unknown[]) =>
      apiMocks.switchManagedTargetProvider(...args),
  },
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
  toast: { error: vi.fn() },
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
  apiMocks.switchProvider.mockReset().mockResolvedValue({ warnings: [] });
  apiMocks.switchManagedTargetProvider.mockReset().mockResolvedValue({});
  apiMocks.updateTrayMenu.mockReset().mockResolvedValue(true);
});

describe("useSwitchProviderMutation", () => {
  it("switches only the selected Codex Managed Target", async () => {
    const { wrapper, invalidateSpy } = createWrapper();
    const { result } = renderHook(
      () => useSwitchProviderMutation("codex", "wsl-ubuntu"),
      { wrapper },
    );

    await act(async () => {
      await result.current.mutateAsync("provider-b");
    });

    expect(apiMocks.switchManagedTargetProvider).toHaveBeenCalledWith(
      "wsl-ubuntu",
      "provider-b",
    );
    expect(apiMocks.switchProvider).not.toHaveBeenCalled();
    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: ["managed-targets"],
    });
  });

  it("keeps the legacy switch path for other applications", async () => {
    const { wrapper } = createWrapper();
    const { result } = renderHook(
      () => useSwitchProviderMutation("claude", "ignored-target"),
      { wrapper },
    );

    await act(async () => {
      await result.current.mutateAsync("provider-a");
    });

    expect(apiMocks.switchProvider).toHaveBeenCalledWith(
      "provider-a",
      "claude",
    );
    expect(apiMocks.switchManagedTargetProvider).not.toHaveBeenCalled();
  });
});
