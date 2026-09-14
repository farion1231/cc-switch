import { describe, expect, it } from "vitest";
import { parseWindowToken } from "@/components/providers/forms/hooks/useModelState";

describe("parseWindowToken", () => {
  it("parses pure numbers and K/M suffixes", () => {
    expect(parseWindowToken("500000")).toBe(500000);
    expect(parseWindowToken("200K")).toBe(200000);
    expect(parseWindowToken("1M")).toBe(1000000);
    expect(parseWindowToken(" 2m ")).toBe(2000000);
  });

  it("rejects empty, zero and invalid input", () => {
    expect(parseWindowToken("")).toBeUndefined();
    expect(parseWindowToken("   ")).toBeUndefined();
    expect(parseWindowToken("0")).toBeUndefined();
    expect(parseWindowToken("0K")).toBeUndefined();
    expect(parseWindowToken("1.5M")).toBeUndefined();
    expect(parseWindowToken("12abc")).toBeUndefined();
    expect(parseWindowToken("1,000,000")).toBeUndefined();
  });

  it("rejects values beyond TOML i64 / safe-integer range", () => {
    expect(parseWindowToken("99999999999999999999")).toBeUndefined();
    expect(parseWindowToken("99999999999999M")).toBeUndefined();
    expect(parseWindowToken("9223372036854775808")).toBeUndefined();
    expect(parseWindowToken("9007199254740991")).toBe(9007199254740991);
  });
});
