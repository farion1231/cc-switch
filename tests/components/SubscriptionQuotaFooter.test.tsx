import { fireEvent, render, screen, within } from "@testing-library/react";
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
  refetch: () => unknown = vi.fn(),
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
        refetch={refetch}
        appIdForExpiredHint="claude"
        inline={inline}
      />
    </I18nextProvider>,
  );
}

/** 额度句子里数值单独包了一层 span（只给数值上色），按整句文字找最内层的元素 */
const sentence = (text: string) => (_: string, node: Element | null) =>
  node?.textContent === text &&
  !Array.from(node.children).some((child) => child.textContent === text);

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
    // 第一行固定是 5 小时，哪怕它剩得最多；其余两档并成一行，快用完的那段单独标橙
    const lines = screen.getByRole("button").children;
    expect(lines[0]).toHaveTextContent("5 小时剩余 88%");
    expect(lines[1]).toHaveTextContent("每周 75% · Fable 5%");
    // 只有数值上色，档名跟着外层灰字
    expect(screen.getByText("75%")).toHaveClass("text-green-600");
    expect(screen.getByText("5%")).toHaveClass("text-orange-500");
    expect(screen.getByText("88%")).toHaveClass("text-green-600");
    expect(screen.getByText("88%").parentElement).toHaveClass("text-fg-2");
    // 重置倒计时直接写在每行后面：5 小时那行没有重置时间，留空占位；合并行写最近的那次
    expect(screen.getByText("2d12h")).toBeInTheDocument();
    expect(
      screen.getAllByText(/后重置$/).map((node) => node.textContent),
    ).toEqual(["2d12h后重置"]);
    // 悬停说明照旧逐档写全
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
    expect(screen.getByText("已用完")).toHaveClass("text-red-500");
    // 展开时重置时间直接写在数值后面
    expect(screen.getByText("2d12h后重置")).toBeInTheDocument();
  });

  it("shows an unused Fable limit in the normal color", () => {
    renderQuota([{ name: "seven_day_fable", utilization: 0, resetsAt: null }]);
    expect(screen.getByText("100%")).toHaveClass("text-green-600");
  });

  it("keeps legacy quotas visible without inventing a Fable limit", () => {
    renderQuota(baseTiers);
    expect(screen.getByText(sentence("5 小时剩余 88%"))).toBeInTheDocument();
    expect(screen.getByText(sentence("每周剩余 75%"))).toBeInTheDocument();
    expect(screen.queryByText(/Fable/)).not.toBeInTheDocument();
  });

  it.each([
    ["zh-TW", "Fable 剩餘 63%"],
    ["en", "Fable 63% left"],
    ["ja", "Fable 残り 63%"],
  ])("localizes the Fable line in %s", async (language, text) => {
    await i18n.changeLanguage(language);
    renderQuota([{ name: "seven_day_fable", utilization: 37, resetsAt: null }]);
    expect(screen.getByText(sentence(text))).toBeInTheDocument();
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

describe("ChatGPT saved limit resets", () => {
  const codex = (inline: boolean, refetch?: () => unknown) =>
    renderQuota(
      baseTiers,
      inline,
      {
        tool: "codex",
        resetCredits: {
          expiresAt: ["2026-09-20T00:00:00Z", null],
        },
      },
      refetch,
    );

  it("rides along with the weekly tier on the card", () => {
    codex(true);
    expect(screen.getByText(sentence("5 小时剩余 88%"))).toBeInTheDocument();
    expect(screen.getByText(sentence("重置 2 次"))).toBeInTheDocument();
    expect(
      screen
        .getByRole("button", { name: /点击重新查询/ })
        .getAttribute("title"),
    ).toContain("存下的限额重置剩余 2 次");
  });

  it("drops down the expiries from the resets, while the usage still refreshes", () => {
    const refetch = vi.fn();
    codex(true, refetch);

    // 点重置次数：开下拉，不重查
    fireEvent.click(
      screen.getByRole("button", { name: "查看 2 次重置各自的到期时间" }),
    );
    expect(refetch).not.toHaveBeenCalled();
    const dialog = screen.getByRole("dialog", { name: "存下的限额重置" });
    expect(within(dialog).getAllByRole("listitem")).toHaveLength(2);
    expect(within(dialog).getByText("不会过期")).toBeInTheDocument();

    // 第一行和同一行的「每周」照旧点了重查
    fireEvent.click(screen.getByRole("button", { name: /点击重新查询/ }));
    expect(refetch).toHaveBeenCalledTimes(1);
  });

  it("gets its own row in the expanded view, with the earliest expiry instead of a bar", () => {
    codex(false);
    expect(screen.getByText("重置")).toBeInTheDocument();
    expect(screen.getByText("剩余 2 次")).toBeInTheDocument();
    expect(screen.getByText(/到期$/)).toBeInTheDocument();
    // 只有两档画额度条
    expect(screen.getAllByRole("meter")).toHaveLength(2);
  });

  it("stays out of sight when nothing is saved", () => {
    renderQuota(baseTiers, true, {
      tool: "codex",
      resetCredits: { expiresAt: [] },
    });
    expect(screen.queryByText(/重置/)).not.toBeInTheDocument();
  });
});

describe("ChatGPT Credits balance", () => {
  const credits = {
    tool: "codex",
    creditsBalance: 62500,
    resetCredits: { expiresAt: ["2026-09-20T00:00:00Z", null] },
  };
  const usedUp: QuotaTier[] = [
    { name: "five_hour", utilization: 100, resetsAt: null },
    { name: "seven_day", utilization: 25, resetsAt: null },
  ];

  it("gets its own row in the expanded view: credits where the bar would be, dollars last", () => {
    renderQuota(baseTiers, false, credits);
    expect(screen.getByText("Credits")).toBeInTheDocument();
    expect(screen.getByText("62,500")).toBeInTheDocument();
    expect(screen.getByText("约 $2500")).toBeInTheDocument();
    // 只有两档画额度条
    expect(screen.getAllByRole("meter")).toHaveLength(2);
  });

  it("stays off the card while no tier is used up", () => {
    renderQuota(baseTiers, true, credits);
    expect(screen.queryByText(/\$2500/)).not.toBeInTheDocument();
    expect(screen.getByText(sentence("重置 2 次"))).toBeInTheDocument();
  });

  it("shows up on the card in dollars once a tier is used up, ahead of the resets", () => {
    renderQuota(usedUp, true, credits);
    expect(screen.getByText("$2500")).toBeInTheDocument();
    expect(screen.getByText(sentence("每周 75%"))).toBeInTheDocument();
    // 合并行只放得下两段：余额排在重置次数前面
    expect(screen.queryByText(sentence("重置 2 次"))).not.toBeInTheDocument();
    // 没有下拉段时整列是一个按钮，悬停说明里写全余额
    expect(
      screen.getByRole("button", { name: /\$2500/ }).getAttribute("title"),
    ).toContain("Credits 余额 62,500");
  });

  it("keeps both the balance and the resets when the plan has a single tier", () => {
    renderQuota(
      [{ name: "seven_day", utilization: 100, resetsAt: null }],
      true,
      credits,
    );
    expect(screen.getByText("$2500")).toBeInTheDocument();
    expect(screen.getByText(sentence("重置 2 次"))).toBeInTheDocument();
  });

  it("is listed under the resets when they drop down on the card", () => {
    renderQuota(baseTiers, true, credits);
    fireEvent.click(
      screen.getByRole("button", { name: "查看 2 次重置各自的到期时间" }),
    );
    const dialog = screen.getByRole("dialog", { name: "存下的限额重置" });
    // 两次到期日 + 一条余额
    expect(within(dialog).getAllByRole("listitem")).toHaveLength(3);
    expect(within(dialog).getByText("Credits")).toBeInTheDocument();
    expect(within(dialog).getByText("62,500")).toBeInTheDocument();
    expect(within(dialog).getByText("约 $2500")).toBeInTheDocument();
  });

  it("lets a single saved reset drop down too, so the balance can be seen", () => {
    renderQuota(baseTiers, true, {
      ...credits,
      resetCredits: { expiresAt: ["2026-09-20T00:00:00Z"] },
    });
    fireEvent.click(
      screen.getByRole("button", { name: "查看 1 次重置各自的到期时间" }),
    );
    const dialog = screen.getByRole("dialog", { name: "存下的限额重置" });
    expect(within(dialog).getByText("约 $2500")).toBeInTheDocument();
  });

  it("does not repeat the balance inside the drop-down when expanded", () => {
    renderQuota(baseTiers, false, credits);
    fireEvent.click(
      screen.getByRole("button", { name: "查看 2 次重置各自的到期时间" }),
    );
    const dialog = screen.getByRole("dialog", { name: "存下的限额重置" });
    expect(within(dialog).getAllByRole("listitem")).toHaveLength(2);
  });

  it("stays out of sight without a balance", () => {
    renderQuota(usedUp, false, { tool: "codex" });
    expect(screen.queryByText("Credits")).not.toBeInTheDocument();
  });
});
