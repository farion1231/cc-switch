import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import type { PropsWithChildren } from "react";
import { describe, expect, it, vi } from "vitest";
import { useUsageFirstDate, useUsageTrends } from "@/lib/query/usage";

const invokeMock = vi.hoisted(() =>
  vi.fn(async (command: string) =>
    command === "get_usage_first_date" ? null : [],
  ),
);
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

describe("Hermes aggregate query scope", () => {
  it("forwards Profile/task through real hooks and API and refetches a changed scope", async () => {
    const client = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const wrapper = ({ children }: PropsWithChildren) => (
      <QueryClientProvider client={client}>{children}</QueryClientProvider>
    );
    const { result, rerender, unmount } = renderHook(
      ({ profileName }) => ({
        trends: useUsageTrends(
          { preset: "7d" },
          { appType: "hermes", profileName, task: "main" },
        ),
        firstDate: useUsageFirstDate({
          appType: "hermes",
          profileName,
          task: "main",
        }),
      }),
      { wrapper, initialProps: { profileName: "profile-a" } },
    );
    await waitFor(() => expect(result.current.trends.isSuccess).toBe(true));
    await waitFor(() => expect(result.current.firstDate.isSuccess).toBe(true));
    for (const command of ["get_usage_trends", "get_usage_first_date"]) {
      expect(invokeMock).toHaveBeenCalledWith(
        command,
        expect.objectContaining({
          appType: "hermes",
          profileName: "profile-a",
          task: "main",
        }),
      );
    }
    rerender({ profileName: "profile-b" });
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith(
        "get_usage_trends",
        expect.objectContaining({ profileName: "profile-b", task: "main" }),
      ),
    );
    unmount();
    client.clear();
  });
});
