import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { createInstance } from "i18next";
import { I18nextProvider } from "react-i18next";
import { afterEach, describe, expect, it, vi } from "vitest";
import { UsageHero } from "@/components/usage/UsageHero";
import type { UsageSummaryByApp } from "@/types/usage";
import en from "@/i18n/locales/en.json";
import zh from "@/i18n/locales/zh.json";
import zhTW from "@/i18n/locales/zh-TW.json";
import ja from "@/i18n/locales/ja.json";

const useSummaryMock = vi.hoisted(() => vi.fn());
vi.mock("@/lib/query/usage", () => ({
  useUsageSummaryByApp: (...args: unknown[]) => useSummaryMock(...args),
}));
afterEach(cleanup);

const summary = {
  totalRequests: 2,
  totalCost: "1",
  totalInputTokens: 10,
  totalOutputTokens: 5,
  totalCacheCreationTokens: 0,
  totalCacheReadTokens: 0,
  totalCacheWriteTokens: 0,
  totalReasoningTokens: 0,
  realTotalTokens: 15,
  cacheHitRate: 0,
  successRate: 0,
};

async function renderExpanded(
  lng: string,
  items: UsageSummaryByApp[],
  appType?: string,
) {
  const i18n = createInstance();
  await i18n.init({
    lng,
    fallbackLng: "en",
    resources: {
      en: { translation: en },
      zh: { translation: zh },
      "zh-TW": { translation: zhTW },
      ja: { translation: ja },
    },
  });
  useSummaryMock.mockReturnValue({ data: items, isLoading: false });
  render(
    <I18nextProvider i18n={i18n}>
      <UsageHero
        range={{ preset: "today" }}
        appType={appType}
        refreshIntervalMs={0}
      />
    </I18nextProvider>,
  );
  fireEvent.click(
    screen.getByRole("button", { name: i18n.t("usage.metrics.more") }),
  );
  return i18n;
}

describe("UsageHero reasoning availability and real translations", () => {
  it.each([
    ["en", "Reasoning tokens"],
    ["zh", "推理 Token"],
    ["zh-TW", "推理 Token"],
    ["ja", "推論トークン"],
  ])(
    "renders the %s reasoning label for known Hermes zero",
    async (lng, label) => {
      const i18n = await renderExpanded(
        lng,
        [{ appType: "hermes", summary }],
        "hermes",
      );
      expect(
        i18n.getResource(lng, "translation", "usage.hermes.reasoningTokens"),
      ).toBe(label);
      expect(screen.getByText(label)).toBeInTheDocument();
      expect(
        screen.queryByText("usage.hermes.reasoningTokens"),
      ).not.toBeInTheDocument();
    },
  );

  it.each(["claude", "codex", undefined])(
    "hides unknown reasoning for %s without Hermes",
    async (appType) => {
      await renderExpanded(
        "en",
        [
          { appType: "claude", summary },
          { appType: "codex", summary },
        ],
        appType,
      );
      expect(screen.queryByText("Reasoning tokens")).not.toBeInTheDocument();
      expect(
        screen.queryByText("usage.hermes.reasoningTokens"),
      ).not.toBeInTheDocument();
    },
  );

  it("shows reported reasoning in mixed All without hiding known Hermes zero", async () => {
    await renderExpanded("en", [
      { appType: "claude", summary },
      { appType: "hermes", summary: { ...summary, totalReasoningTokens: 7 } },
    ]);
    expect(screen.getByText("Reasoning tokens")).toBeInTheDocument();
    expect(screen.getByText("7")).toBeInTheDocument();
  });

  it("hides reasoning when Hermes does not report the metric", async () => {
    await renderExpanded(
      "en",
      [
        {
          appType: "hermes",
          summary: { ...summary, totalReasoningTokens: undefined },
        },
      ],
      "hermes",
    );
    expect(screen.queryByText("Reasoning tokens")).not.toBeInTheDocument();
  });

  it("does not mistake ordinary-source zero for reported Hermes reasoning in All", async () => {
    await renderExpanded("en", [
      { appType: "claude", summary },
      {
        appType: "hermes",
        summary: { ...summary, totalReasoningTokens: undefined },
      },
    ]);
    expect(screen.queryByText("Reasoning tokens")).not.toBeInTheDocument();
  });
});
