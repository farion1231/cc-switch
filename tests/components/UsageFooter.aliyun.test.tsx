import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi, beforeEach } from "vitest";
import UsageFooter from "@/components/UsageFooter";
import type { Provider } from "@/types";

const useUsageQueryMock = vi.fn();

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: { defaultValue?: string }) =>
      options?.defaultValue ?? key,
  }),
}));

vi.mock("@/lib/query/queries", () => ({
  useUsageQuery: (...args: unknown[]) => useUsageQueryMock(...args),
}));

describe("UsageFooter aliyun balance display", () => {
  beforeEach(() => {
    useUsageQueryMock.mockReset();
    useUsageQueryMock.mockReturnValue({
      data: {
        success: true,
        data: [
          {
            planName: "Alibaba Cloud",
            remaining: 5.43,
            total: 0,
            used: 0,
            unit: "CNY",
            extra: "CreditAmount=0; AvailableCashAmount=5.43; QuotaLimit=0",
          },
        ],
      },
      isFetching: false,
      lastQueriedAt: undefined,
      refetch: vi.fn(),
    });
  });

  it("renders only the cash label and amount", () => {
    const provider = {
      id: "aliyun",
      name: "Alibaba Cloud",
      category: "official",
      meta: {
        usage_script: {
          templateType: "balance",
        },
      },
      settingsConfig: {},
    } as Provider;

    render(
      <UsageFooter
        provider={provider}
        providerId="aliyun"
        appId="claude"
        usageEnabled
        isCurrent
      />,
    );

    expect(screen.getByText("usage.availableCashAmount")).toBeInTheDocument();
    expect(screen.getByText("5.43")).toBeInTheDocument();
    expect(screen.getByText("CNY")).toBeInTheDocument();
    expect(screen.queryByText("Remaining:")).not.toBeInTheDocument();
    expect(screen.queryByText(/CreditAmount=/)).not.toBeInTheDocument();
  });
});
