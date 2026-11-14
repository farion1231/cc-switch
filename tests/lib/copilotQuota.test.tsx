import type { ReactNode } from "react";
import type { TFunction } from "i18next";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

const copilotGetUsageForAccount = vi.hoisted(() => vi.fn());

vi.mock("@/lib/api/copilot", () => ({
  copilotGetUsage: vi.fn(),
  copilotGetUsageForAccount,
}));

import {
  copilotPlanLabel,
  useCopilotQuota,
  type CopilotQuotaTier,
} from "@/lib/query/copilot";
import { copilotQuotaRows } from "@/components/CopilotQuotaFooter";

const detail = (
  entitlement: number,
  remaining: number,
  unlimited = false,
  has_quota = true,
) => ({
  entitlement,
  remaining,
  percent_remaining: entitlement ? (remaining / entitlement) * 100 : 100,
  unlimited,
  has_quota,
});

function wrapper({ children }: { children: ReactNode }) {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return <QueryClientProvider client={client}>{children}</QueryClientProvider>;
}

const t = ((key: string, params?: Record<string, unknown>) =>
  params ? `${key} ${JSON.stringify(params)}` : key) as unknown as TFunction;

describe("useCopilotQuota", () => {
  it("marks Free's Credits as not included and keeps its capped quotas", async () => {
    // 真实 Free 号的响应：Credits 总额 0、has_quota=false；补全 2000、聊天 200
    copilotGetUsageForAccount.mockResolvedValue({
      copilot_plan: "individual",
      access_type_sku: "free_limited_copilot",
      quota_reset_date: "2026-12-01",
      quota_snapshots: {
        premium_interactions: detail(0, 0, false, false),
        completions: detail(2000, 500),
        chat: detail(200, 200),
      },
    });

    const { result } = renderHook(() => useCopilotQuota("acc-free"), {
      wrapper,
    });
    await waitFor(() => expect(result.current.data).toBeDefined());

    expect(result.current.data!.plan).toBe("Free");
    const tiers = result.current.data!.tiers;
    expect(tiers.map((tier) => [tier.name, tier.included])).toEqual([
      ["premium", false],
      ["completions", true],
      ["chat", true],
    ]);
    expect(tiers[1]).toMatchObject({
      remaining: 500,
      entitlement: 2000,
      utilization: 75,
    });
  });

  it("drops a zero-entitlement quota when the response has no has_quota", async () => {
    copilotGetUsageForAccount.mockResolvedValue({
      copilot_plan: "individual",
      quota_reset_date: "2026-12-01",
      quota_snapshots: {
        premium_interactions: {
          entitlement: 0,
          remaining: 0,
          percent_remaining: 0,
          unlimited: false,
        },
        completions: detail(2000, 2000),
        chat: detail(0, 0, true),
      },
    });

    const { result } = renderHook(() => useCopilotQuota("acc-old"), {
      wrapper,
    });
    await waitFor(() => expect(result.current.data).toBeDefined());

    expect(result.current.data!.tiers.map((tier) => tier.name)).toEqual([
      "completions",
    ]);
  });

  it("only keeps premium requests on paid plans", async () => {
    copilotGetUsageForAccount.mockResolvedValue({
      copilot_plan: "individual_max",
      quota_reset_date: "2026-12-01",
      quota_snapshots: {
        premium_interactions: detail(20000, 20000),
        completions: detail(0, 0, true),
        chat: detail(0, 0, true),
      },
    });

    const { result } = renderHook(() => useCopilotQuota("acc-max"), {
      wrapper,
    });
    await waitFor(() => expect(result.current.data).toBeDefined());

    expect(result.current.data!.tiers.map((tier) => tier.name)).toEqual([
      "premium",
    ]);
  });
});

describe("copilotPlanLabel", () => {
  it("reads Free and Student from the SKU before copilot_plan", () => {
    expect(copilotPlanLabel("free_limited_copilot", "individual")).toBe("Free");
    expect(copilotPlanLabel("free_educational_quota", "individual")).toBe(
      "Student",
    );
    expect(copilotPlanLabel("monthly_subscriber_quota", "individual")).toBe(
      "Pro",
    );
    expect(
      copilotPlanLabel("plus_monthly_subscriber_quota", "individual_pro"),
    ).toBe("Pro+");
    expect(
      copilotPlanLabel("max_monthly_subscriber_quota", "individual_max"),
    ).toBe("Max");
    expect(copilotPlanLabel("copilot_business_seat", "business")).toBe(
      "Business",
    );
  });

  it("does not call an individual plan Pro without a SKU", () => {
    // Free 的 copilot_plan 也是 individual，旧版后端不带 SKU 时认不出
    expect(copilotPlanLabel(undefined, "individual")).toBeNull();
    expect(copilotPlanLabel(undefined, "individual_max")).toBe("Max");
    expect(copilotPlanLabel("something_new", "something_new")).toBeNull();
  });
});

describe("copilotQuotaRows", () => {
  const tier = (
    name: CopilotQuotaTier["name"],
    entitlement: number,
    remaining: number,
  ): CopilotQuotaTier => ({
    name,
    entitlement,
    remaining,
    utilization: ((entitlement - remaining) / entitlement) * 100,
    resetsAt: "2026-12-01",
    included: true,
  });

  it("puts remaining and total counts in the hover detail", () => {
    const rows = copilotQuotaRows(
      t,
      [tier("premium", 20000, 19500), tier("completions", 2000, 500)],
      { display: "left", locale: "en-US" },
    );

    expect(rows.map((row) => row.label)).toEqual([
      "subscription.copilotPremium",
      "subscription.copilotCompletions",
    ]);
    expect(rows[0].line.detail).toBe(
      'subscription.copilotCount {"remaining":"19,500","total":"20,000"}',
    );
    expect(rows[1].line.detail).toBe(
      'subscription.copilotCount {"remaining":"500","total":"2,000"}',
    );
  });

  it("writes a not-included tier as a caption with an empty value", () => {
    const [row] = copilotQuotaRows(
      t,
      [
        {
          name: "premium",
          entitlement: 0,
          remaining: 0,
          utilization: 0,
          resetsAt: "2026-12-01",
          included: false,
        },
      ],
      { display: "left", locale: "en-US" },
    );

    expect(row.label).toBe("subscription.copilotPremium");
    expect(row.line).toMatchObject({
      caption: "subscription.copilotNotIncluded",
      value: "",
      tone: "normal",
    });
  });

  it("puts the plan name before the counts when given", () => {
    const [row] = copilotQuotaRows(t, [tier("premium", 50, 50)], {
      display: "left",
      locale: "en-US",
      plan: "free",
    });

    expect(row.line.detail).toBe(
      'free · subscription.copilotCount {"remaining":"50","total":"50"}',
    );
  });
});
