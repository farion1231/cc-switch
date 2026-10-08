import { describe, expect, it } from "vitest";
import type { TFunction } from "i18next";
import {
  balanceLine,
  cardRows,
  creditsLine,
  expiredLine,
  failedLines,
  lineHint,
  pickLines,
  resetCreditsLine,
  tierLine,
  toneForLeft,
} from "@/components/quota/quotaRules";

const t = ((key: string, options?: Record<string, unknown>) => {
  const templates: Record<string, string> = {
    "quota.tierLeft": "{{labelSp}}剩余 {{value}}%",
    "quota.tierUsedUp": "{{labelSp}}已用完",
    "quota.left": "剩余 {{value}}%",
    "quota.balance": "余额 {{value}}",
    "quota.tierShort": "{{label}} {{value}}%",
    "quota.resetCredits.left": "重置剩余 {{count}} 次",
    "quota.resetCredits.short": "重置 {{count}} 次",
    "quota.resetCredits.value": "剩余 {{count}} 次",
    "quota.resetCredits.expiresOn": "{{date}}到期",
    "quota.resetCredits.noExpiry": "不会过期",
    "quota.resetCredits.title": "存下的限额重置",
    "quota.resetCredits.times": "{{count}} 次",
    "quota.resetCredits.inTime": "{{time}}后",
    "subscription.resetsIn": "{{time}}后重置",
    "quota.credits.text": "Credits 约 {{usd}}",
    "quota.credits.usd": "约 {{usd}}",
  };
  const template = templates[key] ?? key;
  return template.replace(/\{\{(\w+)\}\}/g, (_, name) =>
    String(options?.[name] ?? ""),
  );
}) as unknown as TFunction;

describe("quota lines", () => {
  it("writes what is left, quiet until under 10%", () => {
    expect(
      tierLine(
        t,
        { name: "five_hour", utilization: 31, resetsAt: null },
        "5 小时",
      ),
    ).toMatchObject({
      text: "5 小时剩余 69%",
      value: "剩余 69%",
      tone: "normal",
    });
    expect(
      tierLine(
        t,
        { name: "seven_day", utilization: 94, resetsAt: null },
        "每周",
      ),
    ).toMatchObject({ text: "每周剩余 6%", tone: "warning" });
    expect(
      tierLine(
        t,
        { name: "premium", utilization: 100, resetsAt: null },
        "高级请求",
      ),
    ).toMatchObject({ text: "高级请求已用完", tone: "danger" });
    // 档名以字母结尾时隔一个空格
    expect(
      tierLine(
        t,
        { name: "seven_day_opus", utilization: 50, resetsAt: null },
        "每周 Opus",
      ).text,
    ).toBe("每周 Opus 剩余 50%");
    expect(toneForLeft(10)).toBe("normal");
    expect(toneForLeft(9)).toBe("warning");
  });

  it("only colors a balance once it runs out, even when nearly gone", () => {
    expect(balanceLine(t, { remaining: 82.1, unit: "¥" })).toMatchObject({
      text: "余额 82.10 ¥",
      tone: "normal",
    });
    // 不到总额 10% 也不加深，条长照旧按总额算
    expect(balanceLine(t, { remaining: 5, total: 100 })).toMatchObject({
      tone: "normal",
      left: 5,
    });
    expect(balanceLine(t, { remaining: 0 })).toMatchObject({
      text: "quota.balanceUsedUp",
      tone: "danger",
    });
  });

  it("shows a failed query as a red line plus a gray reason", () => {
    expect(failedLines(t, "登录已过期")).toEqual([
      expect.objectContaining({ text: "quota.failed", tone: "danger" }),
      expect.objectContaining({ text: "登录已过期", tone: "muted" }),
    ]);
    expect(expiredLine(t).tone).toBe("danger");
  });

  it("keeps the two tiers with the least left, in their original order", () => {
    const lines = [
      tierLine(t, { name: "a", utilization: 10, resetsAt: null }, "A"),
      tierLine(t, { name: "b", utilization: 95, resetsAt: null }, "B"),
      tierLine(t, { name: "c", utilization: 60, resetsAt: null }, "C"),
    ];
    expect(pickLines(lines, 2).map((line) => line.key)).toEqual(["b", "c"]);
  });

  it("pins the shortest window on the card and merges the other tiers", () => {
    const tier = (name: string, utilization: number) =>
      tierLine(t, { name, utilization, resetsAt: null }, name, name);
    const keys = (rows: ReturnType<typeof cardRows>) =>
      rows.map((row) => row.map((line) => line.key));

    // 两档以内一档一行
    expect(
      keys(cardRows([tier("seven_day", 20), tier("five_hour", 10)])),
    ).toEqual([["seven_day"], ["five_hour"]]);
    // 三档：最短的窗口在第一行，其余按原顺序并成一行
    expect(
      keys(
        cardRows([
          tier("seven_day", 30),
          tier("five_hour", 10),
          tier("seven_day_fable", 95),
        ]),
      ),
    ).toEqual([["five_hour"], ["seven_day", "seven_day_fable"]]);
    // 合并行最多两段，多了留剩余最少的
    expect(
      keys(
        cardRows([
          tier("five_hour", 10),
          tier("seven_day", 30),
          tier("seven_day_opus", 5),
          tier("seven_day_fable", 95),
        ]),
      ),
    ).toEqual([["five_hour"], ["seven_day", "seven_day_fable"]]);
    // 没有短写法的行（余额等）照旧留剩余最少的
    const plain = [
      tierLine(t, { name: "a", utilization: 10, resetsAt: null }, "A"),
      tierLine(t, { name: "b", utilization: 95, resetsAt: null }, "B"),
      tierLine(t, { name: "c", utilization: 60, resetsAt: null }, "C"),
    ];
    expect(keys(cardRows(plain))).toEqual([["b"], ["c"]]);
  });
});

describe("saved limit resets", () => {
  const now = Date.parse("2026-10-04T00:00:00Z");
  const day = 24 * 60 * 60 * 1000;
  const at = (ms: number) => new Date(ms).toISOString();

  it("is hidden when there is no usable reset", () => {
    expect(resetCreditsLine(t, undefined, { now, locale: "en" })).toBeNull();
    expect(
      resetCreditsLine(t, { expiresAt: [] }, { now, locale: "en" }),
    ).toBeNull();
    // 查询之后才过期的也不算
    expect(
      resetCreditsLine(
        t,
        { expiresAt: [at(now - 1000)] },
        { now, locale: "en" },
      ),
    ).toBeNull();
  });

  it("counts what is left and writes the earliest expiry where the bar would be", () => {
    const line = resetCreditsLine(
      t,
      { expiresAt: [at(now - day), at(now + 10 * day), null] },
      { now, locale: "en-US" },
    );
    expect(line).toMatchObject({
      text: "重置剩余 2 次",
      short: "重置 2 次",
      value: "剩余 2 次",
      tone: "normal",
      left: Infinity,
    });
    expect(line?.caption).toMatch(/到期$/);
    expect(
      resetCreditsLine(t, { expiresAt: [null] }, { now, locale: "en" }),
    ).toMatchObject({ caption: "不会过期", tone: "normal" });
  });

  it("lists every expiry once there is more than one, same day grouped", () => {
    // 只有一次：行里写全了，不给明细
    expect(
      resetCreditsLine(
        t,
        { expiresAt: [at(now + 10 * day)] },
        { now, locale: "en" },
      )?.breakdown,
    ).toBeUndefined();

    const soon = now + 2 * day;
    const later = now + 20 * day;
    const breakdown = resetCreditsLine(
      t,
      { expiresAt: [at(soon), at(soon + 60_000), at(later), null] },
      { now, locale: "en-US" },
    )?.breakdown;
    expect(breakdown?.title).toBe("存下的限额重置");
    expect(
      breakdown?.items.map(({ hint, value, tone }) => [hint, value, tone]),
    ).toEqual([
      ["2d0h后", "2 次", "warning"],
      ["20d0h后", "1 次", "normal"],
      [undefined, "1 次", "normal"],
    ]);
    expect(breakdown?.items[2].label).toBe("不会过期");
  });

  it("stands out when the earliest one expires within three days", () => {
    expect(
      resetCreditsLine(
        t,
        { expiresAt: [at(now + 2 * day)] },
        { now, locale: "en" },
      )?.tone,
    ).toBe("warning");
  });

  it("joins the weekly tier on the card's second row", () => {
    const fiveHour = tierLine(
      t,
      { name: "five_hour", utilization: 18, resetsAt: null },
      "5 小时",
      "5 小时",
    );
    const weekly = tierLine(
      t,
      { name: "seven_day", utilization: 36, resetsAt: null },
      "每周",
      "每周",
    );
    const resets = resetCreditsLine(
      t,
      { expiresAt: [null] },
      { now, locale: "en" },
    )!;
    const rows = cardRows([fiveHour, weekly, resets]);
    expect(rows.map((row) => row.map((line) => line.key))).toEqual([
      ["five_hour"],
      ["seven_day", "reset_credits"],
    ]);
    // 只有一档时各占一行，写全称
    expect(cardRows([weekly, resets]).map((row) => row[0].text)).toEqual([
      "每周剩余 64%",
      "重置剩余 1 次",
    ]);
  });
});

describe("reset time in hints", () => {
  const now = Date.parse("2026-10-04T10:00:00Z");
  const fiveHour = tierLine(
    t,
    {
      name: "five_hour",
      utilization: 31,
      resetsAt: "2026-10-04T12:30:00Z",
    },
    "5 小时",
  );

  it("counts down from the render time, not when the line was built", () => {
    expect(fiveHour.detail).toBeUndefined();
    expect(lineHint(t, fiveHour, now)).toBe("5 小时 · 2h30m后重置");
    expect(lineHint(t, fiveHour, now + 60 * 60 * 1000)).toBe(
      "5 小时 · 1h30m后重置",
    );
  });

  it("keeps the extra detail in front and falls back to the line itself", () => {
    expect(lineHint(t, { ...fiveHour, detail: "Pro" }, now)).toBe(
      "Pro · 5 小时 · 2h30m后重置",
    );
    // 重置时间已过：只剩这行本身
    expect(lineHint(t, fiveHour, Date.parse("2026-10-05T00:00:00Z"))).toBe(
      "5 小时剩余 69%",
    );
  });
});

describe("Codex Credits balance", () => {
  it("is hidden without a positive balance", () => {
    expect(creditsLine(t, undefined, { locale: "en" })).toBeNull();
    expect(creditsLine(t, null, { locale: "en" })).toBeNull();
    expect(creditsLine(t, 0, { locale: "en" })).toBeNull();
    expect(creditsLine(t, Number.NaN, { locale: "en" })).toBeNull();
  });

  it("counts credits and converts to whole dollars at $0.04 each", () => {
    const line = creditsLine(t, 62500, { locale: "en" })!;
    expect(line.caption).toBe("62,500");
    expect(line.value).toBe("约 $2500");
    expect(line.short).toBe("$2500");
    expect(line.text).toBe("Credits 约 $2500");
    expect(line.left).toBe(Infinity);
    expect(line.tone).toBe("normal");
    expect(creditsLine(t, 1234.5, { locale: "en" })!.short).toBe("$49");
  });

  it("says under a dollar instead of rounding a small balance to zero", () => {
    expect(creditsLine(t, 10, { locale: "en" })!.short).toBe("<$1");
  });
});
