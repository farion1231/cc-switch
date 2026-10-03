import { render, screen } from "@testing-library/react";
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

function renderQuota(
  tiers: QuotaTier[],
  inline = true,
  overrides: Partial<SubscriptionQuota> = {},
) {
  const quota: SubscriptionQuota = {
    tool: "claude",
    credentialStatus: "valid",
    credentialMessage: null,
    success: true,
    tiers,
    extraUsage: null,
    error: null,
    queriedAt: now,
    ...overrides,
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
  it("pins the shortest window first and merges the rest into one line", () => {
    renderQuota([
      ...baseTiers,
      {
        name: "seven_day_fable",
        utilization: 95,
        resetsAt: "2026-09-12T00:00:00Z",
      },
    ]);
    // 第一行固定是 5 小时，哪怕它剩得最多；其余两档并成一行，快用完的那段单独加深
    const lines = screen.getByRole("button").children;
    expect(lines[0]).toHaveTextContent("5 小时剩余 88%");
    expect(lines[1]).toHaveTextContent("每周 75% · Fable 5%");
    expect(screen.getByText("每周 75%")).toHaveClass("text-fg-2");
    expect(screen.getByText("Fable 5%")).toHaveClass(
      "font-medium",
      "text-fg-1",
    );
    // 重置时间在悬停说明里
    expect(screen.getByRole("button").getAttribute("title")).toContain(
      "Fable · 2d12h后重置",
    );
  });

  it.each([
    ["en", "5-hour 88% left", "Wk 75% · Fable 5%"],
    ["ja", "5時間 残り 88%", "週 75% · Fable 5%"],
  ])(
    "uses short tier names on the merged line in %s",
    async (language, first, merged) => {
      await i18n.changeLanguage(language);
      renderQuota([
        ...baseTiers,
        { name: "seven_day_fable", utilization: 95, resetsAt: null },
      ]);
      const lines = screen.getByRole("button").children;
      expect(lines[0]).toHaveTextContent(first);
      expect(lines[1]).toHaveTextContent(merged);
    },
  );

  it("shows every tier as a bar when expanded", () => {
    renderQuota(
      [
        ...baseTiers,
        {
          name: "seven_day_fable",
          utilization: 100,
          resetsAt: "2026-09-12T00:00:00Z",
        },
      ],
      false,
    );
    expect(
      screen.getByRole("meter", { name: "5 小时: 剩余 88%" }),
    ).toHaveAttribute("aria-valuenow", "88");
    expect(screen.getByText("已用完")).toHaveClass("text-danger-text");
    expect(screen.getByText("Fable").closest("div")).toHaveAttribute(
      "title",
      "Fable · 2d12h后重置",
    );
  });

  it("shows an unused Fable limit in the quiet color", () => {
    renderQuota([{ name: "seven_day_fable", utilization: 0, resetsAt: null }]);
    expect(screen.getByText("Fable 剩余 100%")).toHaveClass("text-fg-2");
  });

  it("keeps legacy quotas visible without inventing a Fable limit", () => {
    renderQuota(baseTiers);
    expect(screen.getByText("5 小时剩余 88%")).toBeInTheDocument();
    expect(screen.getByText("每周剩余 75%")).toBeInTheDocument();
    expect(screen.queryByText(/Fable/)).not.toBeInTheDocument();
  });

  it.each([
    ["zh-TW", "Fable 剩餘 63%"],
    ["en", "Fable 63% left"],
    ["ja", "Fable 残り 63%"],
  ])("localizes the Fable line in %s", async (language, text) => {
    await i18n.changeLanguage(language);
    renderQuota([{ name: "seven_day_fable", utilization: 37, resetsAt: null }]);
    expect(screen.getByText(text)).toBeInTheDocument();
  });
});

describe("credential failures", () => {
  const failed = (credentialStatus: SubscriptionQuota["credentialStatus"]) =>
    renderQuota([], true, {
      success: false,
      credentialStatus,
      error: "raw backend message",
    });

  it("says the token is waiting for a refresh, not that the login expired", () => {
    failed("refresh_pending");
    expect(screen.getByText("额度没查到")).toBeInTheDocument();
    expect(screen.getByText("令牌待刷新")).toBeInTheDocument();
    expect(screen.queryByText("登录已过期")).not.toBeInTheDocument();
    expect(screen.queryByText("raw backend message")).not.toBeInTheDocument();
  });

  it("still says the login expired when it really did", () => {
    failed("expired");
    expect(screen.getByText("登录已过期")).toBeInTheDocument();
  });
});
