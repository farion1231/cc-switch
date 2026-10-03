import { describe, expect, it } from "vitest";
import type { TFunction } from "i18next";
import {
  balanceLine,
  cardRows,
  expiredLine,
  failedLines,
  pickLines,
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

  it("only colors a balance without a total once it runs out", () => {
    expect(balanceLine(t, { remaining: 82.1, unit: "¥" })).toMatchObject({
      text: "余额 82.10 ¥",
      tone: "normal",
    });
    expect(balanceLine(t, { remaining: 5, total: 100 }).tone).toBe("warning");
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
