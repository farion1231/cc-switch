import type { ReactNode } from "react";
import { act, renderHook } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it } from "vitest";
import { useAccountQuotaPlans } from "@/lib/query/accountPlans";

function setup() {
  const client = new QueryClient();
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={client}>{children}</QueryClientProvider>
  );
  return { client, wrapper };
}

describe("useAccountQuotaPlans", () => {
  it("reads each account's plan from its quota cache", () => {
    const { client, wrapper } = setup();
    const { result } = renderHook(
      () => useAccountQuotaPlans("codex_oauth", ["a", "b"]),
      { wrapper },
    );
    expect(result.current).toEqual({});

    act(() => {
      client.setQueryData(["codex_oauth", "quota", "a"], {
        success: true,
        planName: "Pro 200",
      });
      client.setQueryData(["codex_oauth", "quota", "b"], { success: true });
    });
    expect(result.current).toEqual({ a: "Pro 200" });
  });

  it("keeps the last plan when a refresh fails", () => {
    const { client, wrapper } = setup();
    const { result } = renderHook(
      () => useAccountQuotaPlans("xai_oauth", ["a"]),
      { wrapper },
    );
    act(() => {
      client.setQueryData(["xai_oauth", "quota", "a"], {
        success: true,
        planName: "SuperGrok",
      });
    });
    act(() => {
      client.setQueryData(["xai_oauth", "quota", "a"], { success: false });
    });
    expect(result.current).toEqual({ a: "SuperGrok" });

    // 查成功但没带套餐：不再显示旧的
    act(() => {
      client.setQueryData(["xai_oauth", "quota", "a"], { success: true });
    });
    expect(result.current).toEqual({});
  });

  it("uses the picker for services with their own shape", () => {
    const { client, wrapper } = setup();
    client.setQueryData(["copilot", "quota", "a"], {
      success: true,
      plan: "Max",
    });
    const { result } = renderHook(
      () =>
        useAccountQuotaPlans(
          "copilot",
          ["a"],
          (data) => (data as { plan: string | null }).plan,
        ),
      { wrapper },
    );
    expect(result.current).toEqual({ a: "Max" });
  });
});
