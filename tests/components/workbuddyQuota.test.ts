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
