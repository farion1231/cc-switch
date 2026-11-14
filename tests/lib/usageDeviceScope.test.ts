import { describe, expect, it } from "vitest";
import { normalizeDevice, usageKeys } from "@/lib/query/usage";
import { isDeviceStale } from "@/lib/query/usageSync";

describe("usage device scope", () => {
  it("keeps local-scope query keys identical to the pre-sync shape", () => {
    expect(
      usageKeys.summary("today", undefined, undefined, {
        appType: "claude",
        providerName: "P",
        model: "m",
      }),
    ).toEqual(["usage", "summary", "today", 0, 0, false, "claude", "P", "m"]);
    expect(
      usageKeys.summary("today", undefined, undefined, {
        appType: "claude",
        device: undefined,
      }),
    ).toHaveLength(9);
  });

  it("separates cache entries per device scope", () => {
    const all = usageKeys.trends("7d", undefined, undefined, { device: "all" });
    const one = usageKeys.trends("7d", undefined, undefined, {
      device: "abc-123",
    });
    expect(all.at(-1)).toBe("device:all");
    expect(one.at(-1)).toBe("device:abc-123");
    expect(usageKeys.detail("r1")).toEqual(["usage", "detail", "r1"]);
    expect(usageKeys.detail("r1", "abc")).toEqual([
      "usage",
      "detail",
      "r1",
      "device:abc",
    ]);
  });

  it("normalizes the local sentinel away", () => {
    expect(normalizeDevice(undefined)).toBeUndefined();
    expect(normalizeDevice("local")).toBeUndefined();
    expect(normalizeDevice("")).toBeUndefined();
    expect(normalizeDevice("all")).toBe("all");
  });

  it("treats never-uploaded and old devices as stale", () => {
    const now = 1_000_000_000_000;
    expect(isDeviceStale({ lastUploadAt: null }, 60, now)).toBe(true);
    expect(isDeviceStale({ lastUploadAt: now / 1000 - 30 }, 60, now)).toBe(
      false,
    );
    expect(isDeviceStale({ lastUploadAt: now / 1000 - 61 }, 60, now)).toBe(
      true,
    );
  });
});
