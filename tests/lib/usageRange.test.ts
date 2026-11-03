import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { resolveUsageRange } from "@/lib/usageRange";

function localStart(nowMs: number, preset: "7d" | "14d" | "30d"): string {
  const { startDate } = resolveUsageRange({ preset }, nowMs);
  const d = new Date(startDate * 1000);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(
    d.getHours(),
  )}:${pad(d.getMinutes())}`;
}

describe("resolveUsageRange multi-day presets across DST", () => {
  const originalTz = process.env.TZ;

  beforeEach(() => {
    process.env.TZ = "America/New_York";
  });

  afterEach(() => {
    if (originalTz === undefined) {
      delete process.env.TZ;
    } else {
      process.env.TZ = originalTz;
    }
  });

  it("does not add an extra day after the spring-forward change", () => {
    const now = new Date(2026, 2, 9, 0, 30).getTime();
    expect(localStart(now, "7d")).toBe("2026-03-03 00:00");
    expect(localStart(now, "14d")).toBe("2026-02-24 00:00");
  });

  it("does not drop a day after the fall-back change", () => {
    const now = new Date(2026, 10, 2, 23, 30).getTime();
    expect(localStart(now, "7d")).toBe("2026-10-27 00:00");
    expect(localStart(now, "30d")).toBe("2026-10-04 00:00");
  });

  it("starts at local midnight even when today's midnight was skipped", () => {
    process.env.TZ = "America/Santiago";
    // 2026-09-06 00:00 does not exist in Santiago; the day starts at 01:00
    const now = new Date(2026, 8, 6, 12, 0).getTime();
    expect(localStart(now, "7d")).toBe("2026-08-31 00:00");
  });

  it("keeps 1d as an elapsed 24-hour window", () => {
    const now = new Date(2026, 2, 9, 0, 30).getTime();
    const { startDate, endDate } = resolveUsageRange({ preset: "1d" }, now);
    expect(endDate - startDate).toBe(24 * 60 * 60);
  });
});
