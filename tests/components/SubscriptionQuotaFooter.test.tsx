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
import {
  SubscriptionQuotaView,
  formatExtraText,
  localClockStr,
} from "@/components/SubscriptionQuotaFooter";
import type { QuotaTier, SubscriptionQuota } from "@/types/subscription";
import zh from "@/i18n/locales/zh.json";
import zhTW from "@/i18n/locales/zh-TW.json";
import en from "@/i18n/locales/en.json";
import ja from "@/i18n/locales/ja.json";

const i18n = createInstance();
const now = Date.parse("2026-09-09T12:00:00Z");
/** Always crosses into another local day, whatever the runner's timezone */
const resetAt = "2026-09-12T00:00:00Z";

const t = (key: string, options?: Record<string, string>): string =>
  i18n.t(key, options) as string;

/**
 * The local-clock label the UI promises for an instant. Assertions stay
 * timezone-independent by deriving the expected digits from the same instant,
 * while still pinning the "MM-DD HH:mm" shape.
 */
function expectedLocalClock(iso: string): string {
  const at = new Date(iso);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${pad(at.getMonth() + 1)}-${pad(at.getDate())} ${pad(at.getHours())}:${pad(at.getMinutes())}`;
}

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
          { name: "seven_day_fable", utilization: 95, resetsAt: resetAt },
        ],
        inline,
      );
      expect(screen.getByText("12%")).toBeInTheDocument();
      expect(screen.getByText("25%")).toBeInTheDocument();
      const row = screen.getByText(/^Fable:?$/).parentElement!;
      expect(within(row).getByText("95%")).toHaveClass("text-red-500");
      const clock = expectedLocalClock(resetAt);
      expect(
        within(row).getByText(inline ? "2d12h" : "2d12h后重置"),
      ).toBeInTheDocument();
      // The local clock stacks underneath the countdown, so the date is its own
      // line instead of the tail a narrow card truncates away first.
      expect(within(row).getByText(clock)).toBeInTheDocument();
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

describe("localClockStr", () => {
  it("renders a reset landing today as a bare local time", () => {
    const sameDay = new Date(now);
    sameDay.setHours(17, 54, 0, 0);
    expect(localClockStr(sameDay.toISOString())).toBe("17:54");
  });

  it("prefixes the date once the reset leaves the current day", () => {
    const later = new Date(now);
    later.setDate(later.getDate() + 3);
    later.setHours(8, 0, 0, 0);
    const pad = (n: number) => String(n).padStart(2, "0");
    expect(localClockStr(later.toISOString())).toBe(
      `${pad(later.getMonth() + 1)}-${pad(later.getDate())} 08:00`,
    );
  });

  it("returns null rather than an Invalid Date label", () => {
    expect(localClockStr(null)).toBeNull();
    expect(localClockStr("not-a-time")).toBeNull();
  });
});

describe("formatExtraText", () => {
  it("turns a bare resets_at ISO string into countdown plus local time", () => {
    expect(formatExtraText(resetAt, t, true)).toBe(
      `2d12h后重置（${expectedLocalClock(resetAt)}）`,
    );
  });

  it("falls back to the plain local moment once the window has passed", () => {
    const past = "2026-09-08T00:00:00Z";
    expect(formatExtraText(past, t, true)).toBe(
      `重置于 ${expectedLocalClock(past)}`,
    );
  });

  it("leaves ISO-shaped custom script text alone unless the template says so", () => {
    // `extra` is free-form display text for script authors, so an ISO string
    // there could be an expiry or a billing date, not a quota reset — only
    // templates that actually fill it from `resets_at` may claim that meaning.
    expect(formatExtraText(resetAt, t)).toBe(resetAt);
    expect(formatExtraText(resetAt, t, false)).toBe(resetAt);
  });

  it("passes through payloads that are not absolute timestamps", () => {
    expect(formatExtraText("$12.34/$20.00", t, true)).toBe("$12.34/$20.00");
    expect(formatExtraText(`{"resetsAt":"${resetAt}"}`, t, true)).toBe(
      `{"resetsAt":"${resetAt}"}`,
    );
    // Naive (offset-less) values keep their raw form: we can't tell which
    // timezone they were written in, so re-labelling them would be a guess.
    expect(formatExtraText("2026-09-12T00:00:00", t, true)).toBe(
      "2026-09-12T00:00:00",
    );
  });

  it("returns null for an absent payload", () => {
    expect(formatExtraText(null, t, true)).toBeNull();
    expect(formatExtraText(undefined, t, true)).toBeNull();
  });
});
