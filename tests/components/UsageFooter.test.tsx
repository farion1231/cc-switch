import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import UsageFooter from "@/components/UsageFooter";
import type { Provider } from "@/types";

const mockUseUsageQuery = vi.fn();

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => {
      const translations: Record<string, string> = {
        "usage.queryFailed": "查询失败",
        "usage.refreshUsage": "刷新用量",
      };
      return translations[key] || key;
    },
  }),
}));

vi.mock("@/lib/query/queries", () => ({
  useUsageQuery: (...args: unknown[]) => mockUseUsageQuery(...args),
}));

describe("UsageFooter", () => {
  const dummyProvider: Provider = {
    id: "test-provider",
    name: "Test Provider",
    settingsConfig: {},
  };

  it("renders inline error state with error text and tooltip when query fails", () => {
    mockUseUsageQuery.mockReturnValue({
      data: {
        success: false,
        error: "Connection refused: 127.0.0.1:7890",
      },
      refetch: vi.fn(),
      isLoading: false,
      isFetching: false,
    });

    render(
      <UsageFooter
        provider={dummyProvider}
        providerId="test-provider"
        appId="claude"
        usageEnabled={true}
        isCurrent={true}
        inline={true}
      />,
    );

    const errorEl = screen.getByText("Connection refused: 127.0.0.1:7890");
    expect(errorEl).toBeInTheDocument();
    expect(errorEl.parentElement).toHaveAttribute(
      "title",
      "Connection refused: 127.0.0.1:7890",
    );
  });

  it("falls back to default translated text when error message is empty in inline mode", () => {
    mockUseUsageQuery.mockReturnValue({
      data: {
        success: false,
        error: "",
      },
      refetch: vi.fn(),
      isLoading: false,
      isFetching: false,
    });

    render(
      <UsageFooter
        provider={dummyProvider}
        providerId="test-provider"
        appId="claude"
        usageEnabled={true}
        isCurrent={true}
        inline={true}
      />,
    );

    expect(screen.getByText("查询失败")).toBeInTheDocument();
  });
});
