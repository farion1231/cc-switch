import type { ReactNode } from "react";
import { act, renderHook, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useManagedAuth } from "@/components/providers/forms/hooks/useManagedAuth";

const apiMocks = vi.hoisted(() => ({
  authGetStatus: vi.fn(),
  authRemoveAccount: vi.fn(),
}));
const toastMocks = vi.hoisted(() => ({
  success: vi.fn(),
}));

vi.mock("@/lib/api", () => ({
  authApi: {
    authGetStatus: (...args: unknown[]) => apiMocks.authGetStatus(...args),
    authRemoveAccount: (...args: unknown[]) =>
      apiMocks.authRemoveAccount(...args),
  },
  settingsApi: {},
}));

vi.mock("sonner", () => ({
  toast: {
    success: toastMocks.success,
  },
}));

function createWrapper() {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  });

  return function Wrapper({ children }: { children: ReactNode }) {
    return (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );
  };
}

describe("useManagedAuth", () => {
  beforeEach(() => {
    apiMocks.authGetStatus.mockReset().mockResolvedValue({
      provider: "codex_oauth",
      authenticated: true,
      default_account_id: "acct-1",
      accounts: [
        {
          id: "acct-1",
          provider: "codex_oauth",
          login: "user@example.com",
          avatar_url: null,
          authenticated_at: 1,
          is_default: true,
          github_domain: "",
          reauth_required: false,
          requires_reauth: false,
        },
      ],
    });
    apiMocks.authRemoveAccount.mockReset().mockResolvedValue(undefined);
  });

  it("shows a success toast after removing an account", async () => {
    const { result } = renderHook(() => useManagedAuth("codex_oauth"), {
      wrapper: createWrapper(),
    });
    await waitFor(() => expect(result.current.isStatusSuccess).toBe(true));

    act(() => result.current.removeAccount("acct-1"));

    await waitFor(() =>
      expect(apiMocks.authRemoveAccount).toHaveBeenCalledWith(
        "codex_oauth",
        "acct-1",
      ),
    );
    await waitFor(() =>
      expect(toastMocks.success).toHaveBeenCalledWith("账号已移除"),
    );
  });
});
