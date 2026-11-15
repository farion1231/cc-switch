import { describe, expect, it } from "vitest";
import {
  REQUEST_LOG_COLUMNS,
  requestLogGroupBreaks,
  requestLogPinnedStart,
  resolveRequestLogColumns,
  setRequestLogColumn,
  TOGGLEABLE_REQUEST_LOG_COLUMNS,
} from "@/components/usage/requestLogColumns";

describe("request log columns", () => {
  it("shows the ten original columns by default and hides the new ones", () => {
    expect(resolveRequestLogColumns(undefined)).toEqual([
      "time",
      "app",
      "provider",
      "model",
      "cost",
      "freshInput",
      "output",
      "cacheRead",
      "speed",
      "firstToken",
    ]);
  });

  it("applies overrides in table order, keeps time, ignores unknown ids", () => {
    expect(
      resolveRequestLogColumns({
        statusCode: true,
        app: false,
        time: false,
        someFutureColumn: true,
      }),
    ).toEqual([
      "time",
      "provider",
      "model",
      "cost",
      "freshInput",
      "output",
      "cacheRead",
      "speed",
      "firstToken",
      "statusCode",
    ]);
    expect(TOGGLEABLE_REQUEST_LOG_COLUMNS.map((c) => c.id)).not.toContain(
      "time",
    );
    expect(TOGGLEABLE_REQUEST_LOG_COLUMNS).toHaveLength(
      REQUEST_LOG_COLUMNS.length - 1,
    );
  });

  it("breaks groups at the first shown column of each group", () => {
    expect([
      ...requestLogGroupBreaks([
        "time",
        "model",
        "cost",
        "output",
        "cacheRead",
        "firstToken",
      ]),
    ]).toEqual(["cost", "output", "firstToken"]);
    expect(requestLogGroupBreaks(["time", "app"]).size).toBe(0);
  });

  it("pins time and app when they lead the table, and only time when app is hidden", () => {
    expect(requestLogPinnedStart(["time", "app", "provider", "model"])).toEqual(
      ["time", "app"],
    );
    expect(requestLogPinnedStart(["time", "provider", "app"])).toEqual([
      "time",
    ]);
  });

  it("stores only columns that differ from their default", () => {
    expect(setRequestLogColumn(undefined, "hitRate", true)).toEqual({
      hitRate: true,
    });
    expect(setRequestLogColumn({ hitRate: true }, "hitRate", false)).toEqual(
      {},
    );
    expect(setRequestLogColumn({}, "app", false)).toEqual({ app: false });
    // 时间列不能关；别的版本写的未知列原样留着
    expect(
      setRequestLogColumn({ someFutureColumn: true }, "time", false),
    ).toEqual({ someFutureColumn: true });
  });
});
