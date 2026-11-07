import { QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import UsageFooter from "@/components/UsageFooter";
import { usageKeys } from "@/lib/query/usage";
import type { Provider, UsageResult } from "@/types";
import { createTestQueryClient } from "../utils/testQueryClient";

function renderFooter(result: UsageResult) {
  const provider: Provider = {
    id: "zhipu",
    name: "Zhipu GLM",
    settingsConfig: {
      env: { ANTHROPIC_BASE_URL: "https://open.bigmodel.cn/api/anthropic" },
    },
    meta: {
      usage_script: {
        enabled: true,
        language: "javascript",
        code: "",
        templateType: "token_plan",
      },
    },
  };
  const queryClient = createTestQueryClient();
  queryClient.setQueryData(usageKeys.script(provider.id, "claude"), result);
  return render(
    <QueryClientProvider client={queryClient}>
      <UsageFooter
        provider={provider}
        providerId={provider.id}
        appId="claude"
        usageEnabled={true}
      />
    </QueryClientProvider>,
  );
}

describe("UsageFooter token plan", () => {
  // 智谱按量账户没有 Coding Plan 时，后端回落成账户余额（#8465）
  it("shows a balance when the token plan query falls back to the account balance", () => {
    renderFooter({
      success: true,
      data: [{ planName: "Zhipu GLM", remaining: 14.68, unit: "CNY" }],
    });

    expect(screen.getByText("Zhipu GLM")).toBeInTheDocument();
    expect(screen.getByText("quota.balance")).toBeInTheDocument();
    expect(screen.queryByText("quota.failed")).not.toBeInTheDocument();
  });

  it("keeps rendering percentage tiers for plan accounts", () => {
    renderFooter({
      success: true,
      data: [
        {
          planName: "five_hour",
          total: 100,
          used: 20,
          remaining: 80,
          unit: "%",
        },
      ],
    });

    expect(screen.queryByText("quota.balance")).not.toBeInTheDocument();
    expect(screen.queryByText("Zhipu GLM")).not.toBeInTheDocument();
  });
});
