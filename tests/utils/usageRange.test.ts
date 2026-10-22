import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import ts from "typescript";
import { describe, expect, it } from "vitest";
import { resolveUsageRange } from "@/lib/usageRange";

// Set TZ before starting Node: changing it inside a Vitest worker does not
// reliably update Date's local timezone on every worker pool/platform.
const rangeModule = ts.transpileModule(
  readFileSync(resolve(__dirname, "../../src/lib/usageRange.ts"), "utf8"),
  { compilerOptions: { module: ts.ModuleKind.ESNext } },
).outputText;

describe("resolveUsageRange", () => {
  it.each([
    [
      "America/New_York",
      "2026-03-09T00:30:00",
      "2026-03-03T00:00:00",
      "2026-02-24T00:00:00",
      "2026-02-08T00:00:00",
    ],
    [
      "America/New_York",
      "2026-11-02T23:30:00",
      "2026-10-27T00:00:00",
      "2026-10-20T00:00:00",
      "2026-10-04T00:00:00",
    ],
    [
      "Europe/Berlin",
      "2026-03-30T00:30:00",
      "2026-03-24T00:00:00",
      "2026-03-17T00:00:00",
      "2026-03-01T00:00:00",
    ],
    [
      "Europe/Berlin",
      "2026-10-26T23:30:00",
      "2026-10-20T00:00:00",
      "2026-10-13T00:00:00",
      "2026-09-27T00:00:00",
    ],
    [
      "Asia/Shanghai",
      "2026-03-09T00:30:00",
      "2026-03-03T00:00:00",
      "2026-02-24T00:00:00",
      "2026-02-08T00:00:00",
    ],
    [
      "UTC",
      "2026-01-02T23:30:00",
      "2025-12-27T00:00:00",
      "2025-12-20T00:00:00",
      "2025-12-04T00:00:00",
    ],
  ])("uses local calendar days in %s at %s", (timeZone, now, ...starts) => {
    const script = `${rangeModule}
      const nowMs = new Date(${JSON.stringify(now)}).getTime();
      const presets = ["7d", "14d", "30d"];
      const starts = ${JSON.stringify(starts)};
      console.log(JSON.stringify({
        actual: presets.map(preset => resolveUsageRange({ preset }, nowMs)),
        expected: starts.map(start => ({
          startDate: new Date(start).getTime() / 1000,
          endDate: nowMs / 1000,
        })),
        rolling: resolveUsageRange({ preset: "1d" }, nowMs),
        today: resolveUsageRange({ preset: "today" }, nowMs),
        midnight: new Date(${JSON.stringify(now.slice(0, 10) + "T00:00:00")}).getTime() / 1000,
      }));
    `;
    const result = JSON.parse(
      execFileSync(process.execPath, ["--input-type=module", "-e", script], {
        env: { ...process.env, TZ: timeZone },
        encoding: "utf8",
      }),
    );
    expect(result.actual).toEqual(result.expected);
    expect(result.rolling.endDate - result.rolling.startDate).toBe(86400);
    expect(result.today.startDate).toBe(result.midnight);
    expect(result.today.endDate).toBe(result.rolling.endDate);
  });

  it("preserves explicit custom timestamps and the all-time range", () => {
    const nowMs = new Date("2026-03-09T00:30:00Z").getTime();
    const custom = {
      preset: "custom" as const,
      customStartDate: 100,
      customEndDate: 200,
    };
    expect(resolveUsageRange(custom, nowMs)).toEqual({
      startDate: 100,
      endDate: 200,
    });
    expect(resolveUsageRange({ ...custom, liveEndTime: true }, nowMs)).toEqual({
      startDate: 100,
      endDate: nowMs / 1000,
    });
    expect(resolveUsageRange({ preset: "all" }, nowMs)).toEqual({
      startDate: 0,
      endDate: nowMs / 1000,
    });
  });
});
