import { render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { CodexBackgroundUsage } from "@/components/usage/CodexBackgroundUsage";
import { usageApi } from "@/lib/api/usage";
import i18n from "i18next";
import en from "@/i18n/locales/en.json";

vi.mock("@/lib/api/usage", () => ({
  usageApi: { getCodexBackgroundUsage: vi.fn() },
}));

function mount() {
  return render(
    <QueryClientProvider
      client={
        new QueryClient({ defaultOptions: { queries: { retry: false } } })
      }
    >
      <CodexBackgroundUsage
        range={{ preset: "custom", customStartDate: 100, customEndDate: 200 }}
        model="test-model"
        refreshIntervalMs={0}
      />
    </QueryClientProvider>,
  );
}

describe("Codex background usage", () => {
  beforeEach(async () => {
    vi.clearAllMocks();
    i18n.addResourceBundle("en", "translation", en, true, true);
    await i18n.changeLanguage("en");
  });
  it("keeps unknown prices explicit and forwards date/model filters", async () => {
    vi.mocked(usageApi.getCodexBackgroundUsage).mockResolvedValue([
      {
        model: "test-model",
        feature: "thread_title",
        status: "success",
        generations: 2,
        inputTokens: 100,
        cachedInputTokens: 80,
        outputTokens: 10,
        estimatedCostUsd: null,
      },
    ]);
    mount();
    expect(await screen.findByText("Unknown price")).toBeInTheDocument();
    expect(
      screen.getByText(
        /Local macOS generation events, excluded from the totals above/,
      ),
    ).toBeInTheDocument();
    expect(usageApi.getCodexBackgroundUsage).toHaveBeenCalledWith(
      100,
      200,
      "test-model",
    );
    expect(screen.getByText("thread_title")).toBeInTheDocument();
  });
  it("does not show failed reads as zero usage", async () => {
    vi.mocked(usageApi.getCodexBackgroundUsage).mockRejectedValue(
      new Error("database unavailable"),
    );
    mount();
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Could not load Codex background usage.",
    );
  });
});
