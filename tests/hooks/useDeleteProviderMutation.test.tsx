import type { ReactNode } from "react";
import { act, renderHook } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useDeleteProviderMutation } from "@/lib/query/mutations";
import { managedAuthStatusQueryKey } from "@/lib/query/managedAuth";
import { subscriptionKeys } from "@/lib/query/subscription";
import { WORKBUDDY_LIVE_KEY } from "@/components/providers/workbuddyOfficial";
import { CODEBUDDY_LIVE_KEY } from "@/lib/query/codebuddy";
import { KIMI_OAUTH_STATUS_KEY } from "@/components/providers/forms/hooks/useKimiOAuth";
import type { AppId } from "@/lib/api/types";

const apiMocks = vi.hoisted(() => ({
  delete: vi.fn(),
  updateTrayMenu: vi.fn(),
}));

vi.mock("@/lib/api", () => ({
  providersApi: {
    delete: (...args: unknown[]) => apiMocks.delete(...args),
    updateTrayMenu: (...args: unknown[]) => apiMocks.updateTrayMenu(...args),
  },
  sessionsApi: {},
  settingsApi: {},
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (_key: string, options?: { defaultValue?: string }) =>
      options?.defaultValue ?? _key,
  }),
}));

vi.mock("sonner", () => ({
  toast: {
    success: vi.fn(),
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
  apiMocks.delete.mockReset().mockResolvedValue(true);
  apiMocks.updateTrayMenu.mockReset().mockResolvedValue(true);
});

describe("useDeleteProviderMutation", () => {
  it.each<{
    appId: AppId;
    expectedKeys: readonly (readonly unknown[])[];
  }>([
    {
      appId: "claude",
      expectedKeys: [
        managedAuthStatusQueryKey("claude_official"),
        subscriptionKeys.quota("claude"),
      ],
    },
    {
      appId: "codex",
      expectedKeys: [
        managedAuthStatusQueryKey("codex_oauth"),
        subscriptionKeys.quota("codex"),
      ],
    },
    {
      appId: "grokbuild",
      expectedKeys: [
        managedAuthStatusQueryKey("xai_oauth"),
        subscriptionKeys.quota("grokbuild"),
      ],
    },
    {
      appId: "agy",
      expectedKeys: [
        managedAuthStatusQueryKey("agy_oauth"),
        subscriptionKeys.quota("agy"),
      ],
    },
    {
      appId: "copilot",
      expectedKeys: [
        managedAuthStatusQueryKey("github_copilot"),
        subscriptionKeys.quota("copilot"),
      ],
    },
    {
      appId: "workbuddy",
      expectedKeys: [
        managedAuthStatusQueryKey("workbuddy_account"),
        WORKBUDDY_LIVE_KEY,
      ],
    },
    {
      appId: "codebuddy",
      expectedKeys: [
        managedAuthStatusQueryKey("codebuddy_oauth"),
        CODEBUDDY_LIVE_KEY,
      ],
    },
    {
      appId: "kimi",
      expectedKeys: [KIMI_OAUTH_STATUS_KEY],
    },
  ])(
    "invalidates Auth Center and live/quota caches when deleting a $appId provider",
    async ({ appId, expectedKeys }) => {
      const { wrapper, invalidateSpy } = createWrapper();
      const { result } = renderHook(() => useDeleteProviderMutation(appId), {
        wrapper,
      });

      await act(async () => {
        await result.current.mutateAsync("card-1");
      });

      expect(apiMocks.delete).toHaveBeenCalledWith("card-1", appId);
      expect(invalidateSpy).toHaveBeenCalledWith({
        queryKey: ["providers", appId],
      });
      for (const queryKey of expectedKeys) {
        expect(invalidateSpy).toHaveBeenCalledWith({ queryKey });
      }
    },
  );
});
