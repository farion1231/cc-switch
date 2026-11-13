import { describe, expect, it } from "vitest";
import type { TFunction } from "i18next";
import type { WorkBuddyQuota } from "@/lib/api/auth";
import { workbuddyCardLines } from "@/components/providers/workbuddyQuota";
import { workbuddyQuotaState } from "@/components/settings/auth/AccountQuota";

const t = ((key: string, options?: Record<string, unknown>) =>
  options && Object.keys(options).length
    ? `${key}:${JSON.stringify(options)}`
    : key) as unknown as TFunction;

const snapshot = {
  planName: "体验版",
  isPaid: false,
  total: 1500,
  remaining: 300,
  used: 1200,
  queriedAt: 1_000_000,
};
const quota = (patch: Partial<WorkBuddyQuota> = {}): WorkBuddyQuota => ({
  snapshot,
  current: true,
  unavailable: null,
  error: null,
  ...patch,
});

describe("workbuddyCardLines", () => {
  it("shows remaining / total credits with a bar tone from the share left", () => {
    const [line, ...rest] = workbuddyCardLines(t, quota(), "en", "left")!;
    expect(rest).toEqual([]);
    expect(line.text).toBe(
      'workbuddy.quota.leftOf:{"remaining":"300","total":"1,500"}',
    );
    expect(line.left).toBe(20);
    expect(line.detail).toContain("体验版");
  });

  it("writes what is used when the quota display says so", () => {
    const [line] = workbuddyCardLines(t, quota(), "en", "used")!;
    expect(line.text).toBe(
      'workbuddy.quota.usedOf:{"used":"1,200","total":"1,500"}',
    );
    expect(line.bar).toBe(80);
  });

  it("adds when the last result was taken while WorkBuddy is not running", () => {
    const lines = workbuddyCardLines(
      t,
      quota({ unavailable: "not_running" }),
      "en",
      "left",
      1_000_000 + 2 * 3600_000,
    )!;
    expect(lines.map((line) => line.key)).toEqual([
      "workbuddy_credits",
      "workbuddy_stale",
    ]);
    expect(lines[0].detail).toContain("workbuddy.quota.staleNotRunning");
    expect(lines[1].text).toContain("quota.updatedAt");
  });

  it("asks to open WorkBuddy when nothing was ever queried, and hides other accounts", () => {
    expect(
      workbuddyCardLines(
        t,
        quota({ snapshot: null, unavailable: "not_running" }),
        "en",
      )?.[0].text,
    ).toBe("workbuddy.quota.openToQuery");
    expect(
      workbuddyCardLines(
        t,
        quota({ snapshot: null, current: false, unavailable: "not_current" }),
        "en",
      ),
    ).toBeNull();
    expect(
      workbuddyCardLines(
        t,
        quota({ snapshot: null, unavailable: "not_signed_in" }),
        "en",
      ),
    ).toBeNull();
  });

  it("falls back to a failure line without a previous result", () => {
    const lines = workbuddyCardLines(
      t,
      quota({ snapshot: null, unavailable: "failed", error: null }),
      "en",
    )!;
    expect(lines[0].text).toBe("quota.failed");
    expect(lines[1].text).toBe("workbuddy.quota.unavailable");
  });
});

describe("workbuddyQuotaState", () => {
  it("labels the row with the plan name", () => {
    const state = workbuddyQuotaState(t, quota(), false, "en");
    expect(state).toMatchObject({
      kind: "rows",
      rows: [{ label: "体验版", line: { value: "300" } }],
    });
  });

  it("says used up when no credits are left", () => {
    const state = workbuddyQuotaState(
      t,
      quota({ snapshot: { ...snapshot, remaining: 0, used: 1500 } }),
      false,
      "en",
    );
    expect(state).toMatchObject({
      kind: "rows",
      rows: [{ line: { value: "quota.usedUp", tone: "danger" } }],
    });
  });

  it("reports a failed query without a previous result", () => {
    expect(
      workbuddyQuotaState(
        t,
        quota({ snapshot: null, unavailable: "failed", error: "boom" }),
        false,
        "en",
      ),
    ).toEqual({ kind: "failed", reason: "boom" });
    expect(workbuddyQuotaState(t, undefined, true, "en")).toEqual({
      kind: "loading",
    });
  });
});

describe("workbuddyExpiryLine", () => {
  const DAY = 24 * 3600_000;
  // 本地时间中午：显示日期按本机时区算，测试不随时区变
  const now = new Date(2026, 10, 12, 12).getTime();
  // 后端按到期先后排好；同一天的合并，过期的（缓存里留下的）不算
  const expiries = [
    { credits: 50, expiresAt: now - DAY },
    { credits: 496.52, expiresAt: now + 18 * DAY },
    { credits: 3.48, expiresAt: now + 18 * DAY + 60_000 },
    { credits: 1500, expiresAt: now + 30 * DAY },
    { credits: 120, expiresAt: null },
  ];
  const withExpiries = (
    list: typeof expiries | undefined,
    patch: Partial<WorkBuddyQuota> = {},
  ) => quota({ snapshot: { ...snapshot, expiries: list }, ...patch });

  it("writes the earliest date as the second card line and lists every date", () => {
    const lines = workbuddyCardLines(
      t,
      withExpiries(expiries),
      "en",
      "left",
      now,
    )!;
    expect(lines.map((line) => line.key)).toEqual([
      "workbuddy_credits",
      "workbuddy_expiry",
    ]);
    const expiry = lines[1];
    expect(expiry.text).toBe(
      'workbuddy.quota.expiresOn:{"credits":"500","date":"Nov 30"}',
    );
    expect(expiry.tone).toBe("plain");
    expect(
      expiry.breakdown?.items.map((item) => [item.label, item.value]),
    ).toEqual([
      ["Nov 30", "500"],
      ["Dec 12", "1,500"],
      ["workbuddy.quota.noExpiry", "120"],
    ]);
  });

  it("turns orange within three days and then beats the stale note", () => {
    const soon = [{ credits: 10, expiresAt: now + 2 * DAY }];
    const lines = workbuddyCardLines(
      t,
      withExpiries(soon, { unavailable: "not_running" }),
      "en",
      "left",
      now,
    )!;
    expect(lines[1].key).toBe("workbuddy_expiry");
    expect(lines[1].tone).toBe("warning");
    expect(lines[1].breakdown).toBeUndefined();
  });

  it("keeps the stale note when nothing expires soon, and shows nothing without dates", () => {
    const stale = workbuddyCardLines(
      t,
      withExpiries(expiries, { unavailable: "not_running" }),
      "en",
      "left",
      now,
    )!;
    expect(stale[1].key).toBe("workbuddy_stale");
    for (const list of [undefined, [], [{ credits: 5, expiresAt: null }]]) {
      const lines = workbuddyCardLines(
        t,
        withExpiries(list),
        "en",
        "left",
        now,
      )!;
      expect(lines.map((line) => line.key)).toEqual(["workbuddy_credits"]);
    }
  });

  it("adds an expiry row in the auth center", () => {
    const state = workbuddyQuotaState(t, withExpiries(expiries), false, "en");
    expect(
      state?.kind === "rows" && state.rows.map((row) => row.label),
    ).toEqual(["体验版", "workbuddy.quota.expiry"]);
  });
});
