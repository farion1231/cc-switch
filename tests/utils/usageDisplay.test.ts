import { describe, expect, it } from "vitest";
import { formatUsageDataSummary } from "@/utils/usageDisplay";

const labels = {
  invalid: "Invalid",
  remaining: "Remaining:",
  used: "Used:",
};

describe("formatUsageDataSummary", () => {
  it("formats used percentage when remaining is omitted", () => {
    expect(
      formatUsageDataSummary(
        {
          planName: "Coco OpenRouter",
          used: 55,
          total: 100,
          unit: "%",
        },
        labels,
      ),
    ).toBe("[Coco OpenRouter] Used: 55%");
  });

  it("formats remaining when present", () => {
    expect(
      formatUsageDataSummary(
        {
          planName: "Balance",
          remaining: 12.5,
          unit: "USD",
        },
        labels,
      ),
    ).toBe("[Balance] Remaining: 12.50 USD");
  });

  it("formats invalid results without requiring quota fields", () => {
    expect(
      formatUsageDataSummary(
        {
          isValid: false,
          invalidMessage: "Unauthorized",
        },
        labels,
      ),
    ).toBe("Unauthorized");
  });

  it("shows remaining before used by default", () => {
    expect(
      formatUsageDataSummary(
        {
          used: 40,
          remaining: 60,
          unit: "USD",
        },
        labels,
      ),
    ).toBe("Remaining: 60 USD / Used: 40 USD");
  });

  it("shows used before remaining when displayOrder is used-first", () => {
    expect(
      formatUsageDataSummary(
        {
          used: 40,
          remaining: 60,
          unit: "USD",
        },
        labels,
        "used-first",
      ),
    ).toBe("Used: 40 USD / Remaining: 60 USD");
  });
});
