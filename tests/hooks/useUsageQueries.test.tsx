import type { ReactNode } from "react";
import { renderHook } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, describe, expect, it, vi } from "vitest";
import { useUsageSummary, usageKeys } from "@/lib/query/usage";

const invokeMock = vi.fn().mockResolvedValue({});

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

function createWrapper() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );

  return { queryClient, wrapper };
}

function getPollingOptions(queryClient: QueryClient) {
  const query = queryClient.getQueryCache().find({
    queryKey: usageKeys.summary(
      "today",
      undefined,
      undefined,
      undefined,
      undefined,
    ),
  });

  return query?.options as
    | {
        refetchInterval?: number | false;
        refetchIntervalInBackground?: boolean;
      }
    | undefined;
}

describe("usage query polling", () => {
  afterEach(() => {
    invokeMock.mockClear();
  });

  // Regression coverage for issue #5996: the dashboard is often left behind
  // the CLI window, so its configured polling must continue after blur.
  it("keeps automatic refresh active while the app is in the background", () => {
    const { queryClient, wrapper } = createWrapper();

    renderHook(
      () =>
        useUsageSummary({ preset: "today" }, undefined, {
          refetchInterval: 5_000,
        }),
      { wrapper },
    );

    const options = getPollingOptions(queryClient);

    expect(options?.refetchInterval).toBe(5_000);
    expect(options?.refetchIntervalInBackground).toBe(true);
  });

  it("does not enable background polling when automatic refresh is off", () => {
    const { queryClient, wrapper } = createWrapper();

    renderHook(
      () =>
        useUsageSummary({ preset: "today" }, undefined, {
          refetchInterval: false,
        }),
      { wrapper },
    );

    const options = getPollingOptions(queryClient);

    expect(options?.refetchInterval).toBe(false);
    expect(options?.refetchIntervalInBackground).toBe(false);
  });
});
