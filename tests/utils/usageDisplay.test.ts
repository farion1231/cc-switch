import { describe, expect, it } from "vitest";
import { formatUsageDataSummary } from "@/utils/usageDisplay";

const labels = {
  invalid: "Invalid",
  remaining: "Remaining:",
  used: "Used:",
};

const mockLabels = { invalid: "无效", remaining: "剩余", used: "已使用" };
const mockData = { total: 100, used: 30, remaining: 70, isValid: true };

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

  // 默认模式（向后兼容）：显示剩余百分比
  it("defaults to remaining mode for backward compatibility", () => {
    expect(formatUsageDataSummary(mockData, mockLabels)).toContain("剩余 70");
    expect(formatUsageDataSummary(mockData, mockLabels)).not.toContain(
      "已使用",
    );
  });

  // used 模式：显示已使用百分比
  it("shows used percentage in used mode", () => {
    expect(formatUsageDataSummary(mockData, mockLabels, "used")).toContain(
      "已使用 30%",
    );
    expect(formatUsageDataSummary(mockData, mockLabels, "used")).not.toContain(
      "剩余",
    );
  });

  // both 模式：同时显示已使用和剩余百分比
  it("shows both used and remaining in both mode", () => {
    const result = formatUsageDataSummary(mockData, mockLabels, "both");
    expect(result).toContain("已使用 30%");
    expect(result).toContain("剩余 70");
  });

  // 无 total 时显示原始值
  it("shows raw used value when total is missing in used mode", () => {
    const noTotal = { used: 50, remaining: undefined };
    expect(
      formatUsageDataSummary(noTotal, mockLabels, "used"),
    ).toContain("已使用 50");
  });

  // 无效数据
  it("shows invalid message regardless of mode", () => {
    const invalid = { isValid: false, invalidMessage: "已过期" };
    expect(formatUsageDataSummary(invalid, mockLabels, "used")).toContain(
      "已过期",
    );
  });
});
