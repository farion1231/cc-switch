import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { Provider, UsageResult } from "@/types";
import UsageFooter from "@/components/UsageFooter";

const useUsageQueryMock = vi.fn();

vi.mock("@/lib/query/queries", () => ({
  useUsageQuery: (...args: unknown[]) => useUsageQueryMock(...args),
}));

const LONG_USAGE_EXTRA =
  "这是用于验证供应商卡片内联用量区域不会覆盖左侧供应商信息的超长自定义用量说明。".repeat(
    4,
  );

const LONG_TOKEN_PLAN_LABEL =
  "最小窗口下用于验证 Token Plan 多套餐显示的超长计划名称".repeat(3);

const baseProvider: Provider = {
  id: "long-usage-provider",
  name: "名称很长的自定义供应商",
  category: "custom",
  settingsConfig: {},
};

function mockUsage(data: UsageResult) {
  useUsageQueryMock.mockReturnValue({
    data,
    isFetching: false,
    isError: false,
    lastQueriedAt: Date.now(),
    refetch: vi.fn(),
  });
}

function renderFooter(provider = baseProvider) {
  return render(
    <UsageFooter
      provider={provider}
      providerId={provider.id}
      appId="claude"
      usageEnabled
      isCurrent
      inline
    />,
  );
}

describe("UsageFooter inline boundary layout", () => {
  it("constrains long custom usage metadata while retaining its full tooltip", () => {
    mockUsage({
      success: true,
      data: [
        {
          used: 10,
          remaining: 90,
          total: 100,
          unit: "CNY",
          extra: LONG_USAGE_EXTRA,
        },
      ],
    });

    const { container } = renderFooter();
    const footer = container.firstElementChild;
    const extra = screen.getByTitle(LONG_USAGE_EXTRA);

    expect(footer).toHaveClass("min-w-0", "max-w-full", "overflow-hidden");
    expect(footer).toHaveClass("sm:max-w-[min(55vw,520px)]");
    expect(extra).toHaveClass("truncate", "max-w-[150px]");
    expect(extra).toHaveTextContent(LONG_USAGE_EXTRA);
  });

  it("keeps the existing token-plan tier information visible for long labels", () => {
    mockUsage({
      success: true,
      data: [
        {
          planName: "weekly_limit",
          used: 25,
          extra: JSON.stringify({
            planLabel: LONG_TOKEN_PLAN_LABEL,
            resetsAt: "2099-12-31T23:59:59Z",
          }),
        },
        {
          planName: "monthly",
          used: 50,
          extra: JSON.stringify({ resetsAt: "2099-12-31T23:59:59Z" }),
        },
      ],
    });

    const tokenPlanProvider: Provider = {
      ...baseProvider,
      meta: {
        usage_script: {
          enabled: true,
          language: "javascript",
          code: "",
          templateType: "token_plan",
        },
      },
    };
    const { container } = renderFooter(tokenPlanProvider);

    expect(screen.getByText(`💰 ${LONG_TOKEN_PLAN_LABEL}`)).toBeVisible();
    expect(screen.getAllByText("subscription.utilization")).toHaveLength(2);
    expect(container.firstElementChild).toHaveClass(
      "min-w-0",
      "max-w-full",
      "overflow-hidden",
    );
  });
});
