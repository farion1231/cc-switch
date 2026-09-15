import { describe, expect, it, vi, beforeEach } from "vitest";
import { renderHook, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { ReactNode } from "react";

const invokeMock = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

import { useDshCurrentState } from "@/lib/query/dsh";

function wrapper({ children }: { children: ReactNode }) {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return <QueryClientProvider client={client}>{children}</QueryClientProvider>;
}

describe("useDshCurrentState", () => {
  beforeEach(() => invokeMock.mockReset());

  it("maps the native state payload", async () => {
    invokeMock.mockResolvedValue({
      providerIds: ["deepseek-official", "k3"],
      currentProviderId: "k3",
      currentModel: "k3",
    });
    const { result } = renderHook(() => useDshCurrentState(true), { wrapper });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(invokeMock).toHaveBeenCalledWith("get_dsh_current_state");
    expect(result.current.data?.providerIds).toContain("k3");
    expect(result.current.data?.currentProviderId).toBe("k3");
  });

  it("is disabled without the flag", () => {
    const { result } = renderHook(() => useDshCurrentState(false), { wrapper });
    expect(result.current.fetchStatus).toBe("idle");
    expect(invokeMock).not.toHaveBeenCalled();
  });
});
