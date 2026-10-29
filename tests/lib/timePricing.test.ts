import { describe, expect, it } from "vitest";
import {
  ALL_WEEKDAYS_MASK,
  WEEKDAY_BITS,
  WEEKDAY_MON_FRI_MASK,
  WEEKDAY_SHORT_NAMES,
  WEEKEND_MASK,
  describeTimeRule,
  emptyTimeRuleDraft,
  formatMinuteOfDay,
  formatUtcOffset,
  formatWeekdayMask,
  isRealCalendarDate,
  parseExcludedDates,
  parseMinuteOfDay,
  parseUtcOffset,
  timeRuleToDraft,
  validateTimeRuleDraft,
  type TimeRuleDraft,
} from "@/lib/timePricing";
import type { ModelPricingTimeRule } from "@/types/usage";

function rule(
  overrides: Partial<ModelPricingTimeRule> = {},
): ModelPricingTimeRule {
  return {
    id: "r1",
    modelId: "deepseek-chat",
    label: "",
    utcOffsetMinutes: 0,
    weekdayMask: WEEKDAY_MON_FRI_MASK,
    startMinute: 60,
    endMinute: 240,
    excludedDates: [],
    inputCostPerMillion: "0.3",
    outputCostPerMillion: "1.2",
    cacheReadCostPerMillion: "0.006",
    cacheCreationCostPerMillion: "0",
    ...overrides,
  };
}

function draft(overrides: Partial<TimeRuleDraft> = {}): TimeRuleDraft {
  return {
    modelId: "deepseek-chat",
    label: "",
    utcOffsetText: "0",
    weekdayMask: WEEKDAY_MON_FRI_MASK,
    startText: "01:00",
    endText: "04:00",
    excludedDatesText: "",
    inputCostPerMillion: "0.3",
    outputCostPerMillion: "1.2",
    cacheReadCostPerMillion: "0.006",
    cacheCreationCostPerMillion: "0",
    ...overrides,
  };
}

describe("formatUtcOffset", () => {
  it("renders zero, positive, negative and half-hour offsets", () => {
    expect(formatUtcOffset(0)).toBe("UTC+00:00");
    expect(formatUtcOffset(480)).toBe("UTC+08:00");
    expect(formatUtcOffset(-300)).toBe("UTC-05:00");
    expect(formatUtcOffset(330)).toBe("UTC+05:30");
    expect(formatUtcOffset(-1439)).toBe("UTC-23:59");
    expect(formatUtcOffset(1439)).toBe("UTC+23:59");
  });
});

describe("parseUtcOffset", () => {
  it("accepts whole minutes only", () => {
    expect(parseUtcOffset("480")).toBe(480);
    expect(parseUtcOffset(" -300 ")).toBe(-300);
    expect(parseUtcOffset("")).toBeNull();
    expect(parseUtcOffset("480.5")).toBeNull();
    expect(parseUtcOffset("abc")).toBeNull();
  });
});

describe("formatMinuteOfDay / parseMinuteOfDay", () => {
  it("round-trips HH:MM and keeps 24:00 for the window's right edge", () => {
    expect(formatMinuteOfDay(0)).toBe("00:00");
    expect(formatMinuteOfDay(60)).toBe("01:00");
    expect(formatMinuteOfDay(1439)).toBe("23:59");
    expect(formatMinuteOfDay(1440)).toBe("24:00");

    expect(parseMinuteOfDay("00:00")).toBe(0);
    expect(parseMinuteOfDay("24:00")).toBe(1440);
    expect(parseMinuteOfDay("22:30")).toBe(1350);
    expect(parseMinuteOfDay("1:05")).toBe(65);
  });

  it("rejects minutes and hours that are out of range", () => {
    expect(parseMinuteOfDay("12:60")).toBeNull();
    expect(parseMinuteOfDay("24:01")).toBeNull();
    expect(parseMinuteOfDay("25:00")).toBeNull();
    expect(parseMinuteOfDay("")).toBeNull();
    expect(parseMinuteOfDay("nope")).toBeNull();
  });
});

describe("formatWeekdayMask", () => {
  it("collapses runs of consecutive days", () => {
    expect(formatWeekdayMask(1)).toBe("Mon");
    expect(formatWeekdayMask(WEEKDAY_MON_FRI_MASK)).toBe("Mon–Fri");
    expect(formatWeekdayMask(WEEKEND_MASK)).toBe("Sat–Sun");
    expect(formatWeekdayMask(ALL_WEEKDAYS_MASK)).toBe("Every day");
    expect(formatWeekdayMask(1 | 4)).toBe("Mon, Wed");
    expect(formatWeekdayMask(2 | 4)).toBe("Tue–Wed");
  });

  it("keeps the Mon…Sun order even across the week boundary", () => {
    // 周五到周日 + 周一在环上是连续的，但渲染保持周一开头、和复选框同序，
    // 宁可少压一段也不打乱顺序
    expect(formatWeekdayMask(16 | 32 | 64 | 1)).toBe("Mon, Fri–Sun");
    expect(formatWeekdayMask(32 | 64 | 1)).toBe("Mon, Sat–Sun");
    expect(formatWeekdayMask(64 | 1)).toBe("Mon, Sun");
  });

  it("names exactly the selected days for every one of the 127 masks", () => {
    // 压区间是容易漏天 / 多天 / 乱序的地方，所以把所有合法掩码都过一遍：
    // 把渲染结果里的区间展开，必须正好等于掩码选中的那几天。
    const indexOfDay = (name: string) =>
      (WEEKDAY_SHORT_NAMES as readonly string[]).indexOf(name);

    for (let mask = 1; mask <= ALL_WEEKDAYS_MASK; mask += 1) {
      const rendered = formatWeekdayMask(mask);
      const expected = WEEKDAY_BITS.map((bit, index) =>
        mask & bit ? index : -1,
      )
        .filter((index) => index >= 0)
        .join(",");

      const listed = new Set<number>();
      if (rendered === "Every day") {
        for (let index = 0; index < 7; index += 1) listed.add(index);
      } else {
        for (const part of rendered.split(", ")) {
          const bounds = /^(\w+)(?:–(\w+))?$/.exec(part);
          expect(bounds, `${mask} → ${rendered} → ${part}`).not.toBeNull();
          const from = indexOfDay(bounds![1]);
          const to = bounds![2] ? indexOfDay(bounds![2]) : from;
          expect(
            from,
            `${mask} → ${rendered} → ${part}`,
          ).toBeGreaterThanOrEqual(0);
          expect(to).toBeGreaterThanOrEqual(from);
          for (let index = from; index <= to; index += 1) listed.add(index);
        }
      }

      expect([...listed].sort((a, b) => a - b).join(","), `mask ${mask}`).toBe(
        expected,
      );
    }
  });

  it("renders an empty mask as a placeholder", () => {
    expect(formatWeekdayMask(0)).toBe("--");
  });
});

describe("isRealCalendarDate", () => {
  it("rejects dates that do not exist", () => {
    expect(isRealCalendarDate("2026-10-01")).toBe(true);
    expect(isRealCalendarDate("2024-02-29")).toBe(true);
    expect(isRealCalendarDate("2026-02-30")).toBe(false);
    expect(isRealCalendarDate("2026-13-01")).toBe(false);
    expect(isRealCalendarDate("2026-1-1")).toBe(false);
    expect(isRealCalendarDate("20261001")).toBe(false);
  });
});

describe("parseExcludedDates", () => {
  it("splits on commas and whitespace", () => {
    expect(parseExcludedDates("")).toEqual([]);
    expect(parseExcludedDates("   ")).toEqual([]);
    expect(parseExcludedDates("2026-10-01")).toEqual(["2026-10-01"]);
    expect(parseExcludedDates("2026-10-01, 2026-10-02")).toEqual([
      "2026-10-01",
      "2026-10-02",
    ]);
    expect(parseExcludedDates("2026-10-01 2026-10-02\n2026-10-03")).toEqual([
      "2026-10-01",
      "2026-10-02",
      "2026-10-03",
    ]);
  });
});

describe("describeTimeRule", () => {
  it("renders a normal window with model, days, time and offset", () => {
    expect(describeTimeRule(rule())).toBe(
      "deepseek-chat · Mon–Fri 01:00–04:00 (UTC+00:00)",
    );
  });

  it("renders a window that wraps past midnight", () => {
    expect(
      describeTimeRule(rule({ startMinute: 22 * 60, endMinute: 8 * 60 })),
    ).toBe("deepseek-chat · Mon–Fri 22:00–08:00 (UTC+00:00)");
  });

  it("renders start == end as the whole 24 hours", () => {
    expect(describeTimeRule(rule({ startMinute: 0, endMinute: 0 }))).toBe(
      "deepseek-chat · Mon–Fri all day (UTC+00:00)",
    );
    expect(
      describeTimeRule(
        rule({ startMinute: 13 * 60 + 7, endMinute: 13 * 60 + 7 }),
      ),
    ).toBe("deepseek-chat · Mon–Fri all day (UTC+00:00)");
  });

  it("renders a single-minute window", () => {
    expect(describeTimeRule(rule({ startMinute: 0, endMinute: 1 }))).toBe(
      "deepseek-chat · Mon–Fri 00:00–00:01 (UTC+00:00)",
    );
  });

  it("renders the window's right edge as 24:00", () => {
    expect(describeTimeRule(rule({ startMinute: 0, endMinute: 1440 }))).toBe(
      "deepseek-chat · Mon–Fri 00:00–24:00 (UTC+00:00)",
    );
  });

  it("distinguishes single weekday, weekday range and every day", () => {
    expect(describeTimeRule(rule({ weekdayMask: 1 }))).toBe(
      "deepseek-chat · Mon 01:00–04:00 (UTC+00:00)",
    );
    expect(describeTimeRule(rule({ weekdayMask: WEEKDAY_MON_FRI_MASK }))).toBe(
      "deepseek-chat · Mon–Fri 01:00–04:00 (UTC+00:00)",
    );
    expect(describeTimeRule(rule({ weekdayMask: ALL_WEEKDAYS_MASK }))).toBe(
      "deepseek-chat · Every day 01:00–04:00 (UTC+00:00)",
    );
    expect(describeTimeRule(rule({ weekdayMask: WEEKEND_MASK }))).toBe(
      "deepseek-chat · Sat–Sun 01:00–04:00 (UTC+00:00)",
    );
  });

  it("renders positive, negative and half-hour offsets", () => {
    expect(describeTimeRule(rule({ utcOffsetMinutes: 480 }))).toBe(
      "deepseek-chat · Mon–Fri 01:00–04:00 (UTC+08:00)",
    );
    expect(describeTimeRule(rule({ utcOffsetMinutes: -480 }))).toBe(
      "deepseek-chat · Mon–Fri 01:00–04:00 (UTC-08:00)",
    );
    expect(describeTimeRule(rule({ utcOffsetMinutes: 330 }))).toBe(
      "deepseek-chat · Mon–Fri 01:00–04:00 (UTC+05:30)",
    );
    expect(describeTimeRule(rule({ utcOffsetMinutes: 0 }))).toBe(
      "deepseek-chat · Mon–Fri 01:00–04:00 (UTC+00:00)",
    );
  });

  it("appends the excluded-dates clause", () => {
    expect(describeTimeRule(rule({ excludedDates: ["2026-10-01"] }))).toBe(
      "deepseek-chat · Mon–Fri 01:00–04:00 (UTC+00:00) · except 2026-10-01",
    );
    expect(
      describeTimeRule(
        rule({ excludedDates: ["2026-10-01", "2026-10-02", "2026-10-03"] }),
      ),
    ).toBe(
      "deepseek-chat · Mon–Fri 01:00–04:00 (UTC+00:00) · except 2026-10-01, 2026-10-02, 2026-10-03",
    );
  });

  it("leads with the label when one is set", () => {
    expect(describeTimeRule(rule({ label: "Peak hours" }))).toBe(
      "Peak hours · deepseek-chat · Mon–Fri 01:00–04:00 (UTC+00:00)",
    );
  });

  it("falls back to a placeholder when the model id is blank", () => {
    expect(describeTimeRule(rule({ modelId: "   " }))).toBe(
      "-- · Mon–Fri 01:00–04:00 (UTC+00:00)",
    );
  });
});

describe("validateTimeRuleDraft", () => {
  it("accepts a well-formed draft", () => {
    expect(validateTimeRuleDraft(draft())).toEqual([]);
  });

  it("requires a model id", () => {
    expect(validateTimeRuleDraft(draft({ modelId: "   " }))).toEqual([
      { field: "modelId", code: "modelIdRequired" },
    ]);
  });

  it("rejects an empty weekday mask", () => {
    expect(validateTimeRuleDraft(draft({ weekdayMask: 0 }))).toEqual([
      { field: "weekdayMask", code: "weekdayMaskEmpty" },
    ]);
  });

  it("rejects a non-integer and an out-of-range UTC offset", () => {
    expect(validateTimeRuleDraft(draft({ utcOffsetText: "8.5" }))).toEqual([
      { field: "utcOffsetMinutes", code: "utcOffsetNotInteger" },
    ]);
    expect(validateTimeRuleDraft(draft({ utcOffsetText: "" }))).toEqual([
      { field: "utcOffsetMinutes", code: "utcOffsetNotInteger" },
    ]);
    expect(validateTimeRuleDraft(draft({ utcOffsetText: "1440" }))).toEqual([
      { field: "utcOffsetMinutes", code: "utcOffsetOutOfRange" },
    ]);
    expect(validateTimeRuleDraft(draft({ utcOffsetText: "-1440" }))).toEqual([
      { field: "utcOffsetMinutes", code: "utcOffsetOutOfRange" },
    ]);
  });

  it("accepts the offset boundaries", () => {
    expect(validateTimeRuleDraft(draft({ utcOffsetText: "1439" }))).toEqual([]);
    expect(validateTimeRuleDraft(draft({ utcOffsetText: "-1439" }))).toEqual(
      [],
    );
  });

  it("rejects out-of-range minutes", () => {
    expect(validateTimeRuleDraft(draft({ startText: "24:00" }))).toEqual([
      { field: "startMinute", code: "startMinuteOutOfRange" },
    ]);
    expect(validateTimeRuleDraft(draft({ startText: "1:2:3" }))).toEqual([
      { field: "startMinute", code: "startMinuteOutOfRange" },
    ]);
    expect(validateTimeRuleDraft(draft({ endText: "24:01" }))).toEqual([
      { field: "endMinute", code: "endMinuteOutOfRange" },
    ]);
    expect(validateTimeRuleDraft(draft({ endText: "12:60" }))).toEqual([
      { field: "endMinute", code: "endMinuteOutOfRange" },
    ]);
  });

  it("accepts 24:00 as the window's right edge", () => {
    expect(validateTimeRuleDraft(draft({ endText: "24:00" }))).toEqual([]);
  });

  it("distinguishes a malformed date from a non-existent one", () => {
    expect(
      validateTimeRuleDraft(draft({ excludedDatesText: "2026-1-1" })),
    ).toEqual([{ field: "excludedDates", code: "excludedDateFormat" }]);
    expect(
      validateTimeRuleDraft(draft({ excludedDatesText: "2026/10/01" })),
    ).toEqual([{ field: "excludedDates", code: "excludedDateFormat" }]);
    expect(
      validateTimeRuleDraft(draft({ excludedDatesText: "2026-02-30" })),
    ).toEqual([{ field: "excludedDates", code: "excludedDateNotReal" }]);
  });

  it("reports every bad date in the list", () => {
    expect(
      validateTimeRuleDraft(
        draft({ excludedDatesText: "2026-10-01, 2026-13-01, 2026-02-30" }),
      ).filter((error) => error.field === "excludedDates"),
    ).toEqual([
      { field: "excludedDates", code: "excludedDateNotReal" },
      { field: "excludedDates", code: "excludedDateNotReal" },
    ]);
  });

  it("rejects prices that are not non-negative decimals", () => {
    for (const field of [
      "inputCostPerMillion",
      "outputCostPerMillion",
      "cacheReadCostPerMillion",
      "cacheCreationCostPerMillion",
    ] as const) {
      expect(validateTimeRuleDraft(draft({ [field]: "-1" }))).toEqual([
        { field, code: "priceInvalid" },
      ]);
      expect(validateTimeRuleDraft(draft({ [field]: "abc" }))).toEqual([
        { field, code: "priceInvalid" },
      ]);
      expect(validateTimeRuleDraft(draft({ [field]: "0.003" }))).toEqual([]);
    }
  });

  it("treats a blank price as missing, not as a valid 0", () => {
    // 留空和「这一档真的免费」必须分开：留空会被静默算成 0，钱就少算了
    for (const field of [
      "inputCostPerMillion",
      "outputCostPerMillion",
      "cacheReadCostPerMillion",
      "cacheCreationCostPerMillion",
    ] as const) {
      expect(validateTimeRuleDraft(draft({ [field]: "" }))).toEqual([
        { field, code: "priceRequired" },
      ]);
      expect(validateTimeRuleDraft(draft({ [field]: "   " }))).toEqual([
        { field, code: "priceRequired" },
      ]);
    }
  });

  it("starts a new draft with blank prices so nothing is silently free", () => {
    const fresh = emptyTimeRuleDraft();
    expect(
      validateTimeRuleDraft(fresh).filter((error) => error.field !== "modelId"),
    ).toEqual([
      { field: "inputCostPerMillion", code: "priceRequired" },
      { field: "outputCostPerMillion", code: "priceRequired" },
      { field: "cacheReadCostPerMillion", code: "priceRequired" },
      { field: "cacheCreationCostPerMillion", code: "priceRequired" },
    ]);
  });
});

describe("timeRuleToDraft", () => {
  it("round-trips a stored rule back into editable text", () => {
    const stored = rule({
      label: "Peak hours",
      utcOffsetMinutes: 480,
      weekdayMask: WEEKDAY_MON_FRI_MASK,
      startMinute: 22 * 60,
      endMinute: 8 * 60,
      excludedDates: ["2026-10-01", "2026-10-02"],
    });

    const restored = timeRuleToDraft(stored);
    expect(restored.utcOffsetText).toBe("480");
    expect(restored.startText).toBe("22:00");
    expect(restored.endText).toBe("08:00");
    expect(restored.excludedDatesText).toBe("2026-10-01, 2026-10-02");
    expect(restored.weekdayMask).toBe(WEEKDAY_MON_FRI_MASK);
  });
});
