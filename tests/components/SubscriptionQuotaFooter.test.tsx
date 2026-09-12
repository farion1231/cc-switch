import { render, screen, within } from "@testing-library/react";
import { createInstance } from "i18next";
import { I18nextProvider, initReactI18next } from "react-i18next";
import {
  afterEach,
  beforeAll,
  beforeEach,
  describe,
  expect,
  it,
  vi,
} from "vitest";
import { SubscriptionQuotaView } from "@/components/SubscriptionQuotaFooter";
import type { QuotaTier, SubscriptionQuota } from "@/types/subscription";
import zh from "@/i18n/locales/zh.json";
import zhTW from "@/i18n/locales/zh-TW.json";
import en from "@/i18n/locales/en.json";
import ja from "@/i18n/locales/ja.json";

const i18n = createInstance();
const now = Date.parse("2026-09-09T12:00:00Z");

beforeAll(async () => {
  await i18n.use(initReactI18next).init({
    lng: "zh",
    resources: {
      zh: { translation: zh },
      "zh-TW": { translation: zhTW },
      en: { translation: en },
      ja: { translation: ja },
    },
    interpolation: { escapeValue: false },
  });
});

beforeEach(async () => {
  vi.spyOn(Date, "now").mockReturnValue(now);
  await i18n.changeLanguage("zh");
});

afterEach(() => vi.restoreAllMocks());

const baseTiers: QuotaTier[] = [
  { name: "five_hour", utilization: 12, resetsAt: null },
  { name: "seven_day", utilization: 25, resetsAt: null },
];

function renderQuota(tiers: QuotaTier[], inline = true) {
  const quota: SubscriptionQuota = {
    tool: "claude",
    credentialStatus: "valid",
    credentialMessage: null,
    success: true,
    tiers,
    extraUsage: null,
    error: null,
    queriedAt: now,
  };
  return render(
    <I18nextProvider i18n={i18n}>
      <SubscriptionQuotaView
        quota={quota}
        loading={false}
        refetch={vi.fn()}
        appIdForExpiredHint="claude"
        inline={inline}
      />
    </I18nextProvider>,
  );
}

describe("Claude Fable subscription quota", () => {
  it.each([true, false])(
    "shows the Fable limit and reset in inline=%s",
    (inline) => {
      renderQuota(
        [
          ...baseTiers,
          {
            name: "seven_day_fable",
            utilization: 95,
            resetsAt: "2026-09-12T00:00:00Z",
          },
        ],
        inline,
      );
      expect(screen.getByText("12%")).toBeInTheDocument();
      expect(screen.getByText("25%")).toBeInTheDocument();
      const row = screen.getByText(/^Fable:?$/).parentElement!;
      expect(within(row).getByText("95%")).toHaveClass("text-red-500");
      expect(
        within(row).getByText(inline ? "2d12h" : "2d12h后重置"),
      ).toBeInTheDocument();
    },
  );

  it("shows an unused Fable limit without a reset countdown", () => {
    renderQuota([{ name: "seven_day_fable", utilization: 0, resetsAt: null }]);
    const row = screen.getByText("Fable:").parentElement!;
    expect(within(row).getByText("0%")).toHaveClass("text-green-600");
    expect(row.querySelector("svg")).toBeNull();
  });

  it("keeps legacy quotas visible without inventing a Fable limit", () => {
    renderQuota(baseTiers);
    expect(screen.getByText("12%")).toBeInTheDocument();
    expect(screen.getByText("25%")).toBeInTheDocument();
    expect(screen.queryByText(/Fable/)).not.toBeInTheDocument();
  });

  it.each([
    ["zh-TW", "Fable:"],
    ["en", "Fable:"],
    ["ja", "Fable:"],
  ])("localizes the Fable label in %s", async (language, label) => {
    await i18n.changeLanguage(language);
    renderQuota([{ name: "seven_day_fable", utilization: 37, resetsAt: null }]);
    expect(screen.getByText(label)).toBeInTheDocument();
  });
});
