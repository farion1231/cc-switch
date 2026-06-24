import { describe, expect, it } from "vitest";

import { formatUsageValue } from "@/utils/usageDisplay";

describe("formatUsageValue", () => {
  describe("non-currency units (integer + thousands separators)", () => {
    it("renders whole token counts without decimals", () => {
      expect(formatUsageValue(5_000_000, "tokens")).toBe("5,000,000");
      expect(formatUsageValue(20, "次")).toBe("20");
      expect(formatUsageValue(100, "points")).toBe("100");
      expect(formatUsageValue(1_234_567, "requests")).toBe("1,234,567");
    });

    it("truncates fractional non-currency quantities to an integer", () => {
      // API returns whole tokens; a stray fractional value should not
      // display `.00`-style noise (issue #4456).
      expect(formatUsageValue(12.5, "tokens")).toBe("13");
      expect(formatUsageValue(5_000_000.99, "tokens")).toBe("5,000,001");
    });

    it("handles zero without decimals", () => {
      expect(formatUsageValue(0, "tokens")).toBe("0");
    });
  });

  describe("currency units (2 decimals)", () => {
    it("keeps 2 decimals for known currency codes and symbols", () => {
      expect(formatUsageValue(12.5, "USD")).toBe("12.50");
      expect(formatUsageValue(12.5, "CNY")).toBe("12.50");
      expect(formatUsageValue(12.5, "EUR")).toBe("12.50");
      expect(formatUsageValue(12.5, "GBP")).toBe("12.50");
      expect(formatUsageValue(12.5, "$")).toBe("12.50");
      expect(formatUsageValue(12.5, "¥")).toBe("12.50");
      expect(formatUsageValue(12.5, "€")).toBe("12.50");
      expect(formatUsageValue(12.5, "£")).toBe("12.50");
    });

    it("adds thousands separators while keeping 2 decimals", () => {
      expect(formatUsageValue(1_234_567.89, "USD")).toBe("1,234,567.89");
      expect(formatUsageValue(1_234_567, "USD")).toBe("1,234,567.00");
    });

    it("rounds currency to 2 decimals", () => {
      expect(formatUsageValue(12.555, "USD")).toBe("12.56");
      expect(formatUsageValue(12.1, "CNY")).toBe("12.10");
    });
  });

  describe("percent unit", () => {
    it("renders percentages with the adaptive integer/2-decimal rule", () => {
      // Mirrors the prior formatValue("%") path which delegated to
      // formatNumber: integers stay integers, fractions keep 2 decimals.
      expect(formatUsageValue(45, "%")).toBe("45%");
      expect(formatUsageValue(45.12, "%")).toBe("45.12%");
    });
  });

  describe("no unit (adaptive)", () => {
    it("keeps integers as integers", () => {
      expect(formatUsageValue(5_000_000)).toBe("5,000,000");
      expect(formatUsageValue(0)).toBe("0");
    });

    it("keeps 2 decimals for fractional values", () => {
      // Preserves prior toFixed(2) behaviour for callers without a unit,
      // now with thousands separators added.
      expect(formatUsageValue(12.5)).toBe("12.50");
      expect(formatUsageValue(1_234.5)).toBe("1,234.50");
    });
  });

  describe("regression guards for issue #4456", () => {
    it("does not show '.00' for integer token quantities", () => {
      // The exact symptom from the issue: 5,000,000 tokens must not
      // render as "5,000,000.00".
      expect(formatUsageValue(5_000_000, "tokens")).not.toContain(".00");
      expect(formatUsageValue(5_000_000, "tokens")).toBe("5,000,000");
    });

    it("adds thousands separators to large token counts", () => {
      // The second symptom: 12000000 must be readable as 12,000,000.
      expect(formatUsageValue(12_000_000, "tokens")).toBe("12,000,000");
    });
  });
});
