import { describe, expect, it } from "vitest";
import { validateConnectionOverride } from "@/utils/connectionOverride";

describe("validateConnectionOverride", () => {
  it("accepts empty value", () => {
    expect(validateConnectionOverride("")).toBeNull();
    expect(validateConnectionOverride("   ")).toBeNull();
  });

  it("accepts valid ipv4 and ipv6 values", () => {
    expect(validateConnectionOverride("1.2.3.4:443")).toBeNull();
    expect(validateConnectionOverride("[2001:db8::1]:8443")).toBeNull();
  });

  it("rejects missing or invalid port", () => {
    expect(validateConnectionOverride("1.2.3.4")).toContain("必须");
    expect(validateConnectionOverride("1.2.3.4:70000")).toContain("1-65535");
  });

  it("rejects non-ip host", () => {
    expect(validateConnectionOverride("example.com:443")).toContain("仅支持");
  });
});
