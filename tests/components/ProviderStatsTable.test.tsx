import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { ProviderStatsTable } from "@/components/usage/ProviderStatsTable";
import type { ProviderStats } from "@/types/usage";

const useProviderStatsMock = vi.hoisted(() => vi.fn());
vi.mock("@/lib/query/usage", () => ({
  useProviderStats: (...args: unknown[]) => useProviderStatsMock(...args),
}));
vi.mock("react-i18next", async () => {
  const zh = (await import("@/i18n/locales/zh.json")).default;
  return {
    useTranslation: () => ({
      t: (key: string, options?: Record<string, unknown> | string) => {
        const name = key.replace("usage.", "") as keyof typeof zh.usage;
        let value = String(zh.usage[name] ?? key);
        if (typeof options === "object") {
          for (const [field, replacement] of Object.entries(options)) {
            value = value.replace(`{{${field}}}`, String(replacement));
          }
        }
        return value;
      },
    }),
  };
});

function showTiming(avgLatencyMs: number | null, latencySampleCount: number) {
  const stat: ProviderStats = {
    providerId: "p1",
    providerName: "Codex (Session)",
    requestCount: 220,
    totalTokens: 1_575_895,
    totalCost: "7.3403836",
    successRate: 100,
    avgLatencyMs,
    latencySampleCount,
  };
  useProviderStatsMock.mockReturnValue({ data: [stat], isLoading: false });
  return render(
    <ProviderStatsTable range={{ preset: "today" }} refreshIntervalMs={0} />,
  );
}

describe("Provider request duration", () => {
  it("shows a precise known-sample mean with its coverage and timing definition", () => {
    showTiming(7_922_752 / 219, 219);
    expect(screen.getByText("平均请求用时")).toBeInTheDocument();
    expect(screen.getByText("36.18s")).toBeInTheDocument();
    expect(screen.getByText("已记录 219/220 条")).toBeInTheDocument();
    expect(screen.getByText("平均请求用时").getAttribute("title")).toContain(
      "响应完成",
    );
  });

  it("shows unknown timing instead of a zero duration when no samples exist", () => {
    showTiming(null, 0);
    expect(screen.getByText("未记录")).toBeInTheDocument();
    expect(screen.getByText("已记录 0/220 条")).toBeInTheDocument();
    expect(screen.queryByText("0ms")).not.toBeInTheDocument();
  });

  it("keeps a genuinely measured zero visible", () => {
    showTiming(0, 1);
    expect(screen.getByText("0ms")).toBeInTheDocument();
    expect(screen.getByText("已记录 1/220 条")).toBeInTheDocument();
  });
});
