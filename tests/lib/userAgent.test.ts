import { describe, it, expect } from "vitest";
import { isValidUserAgentHeader } from "@/lib/userAgent";

// 与后端 parse_custom_user_agent / http::HeaderValue::from_str 的字节级规则对齐：
// 合法 = b>=32 && b!=127 || b=='\t'。即仅控制字符（除 \t 外的 0x00–0x1F 与 0x7F）非法，
// 可见 ASCII 与非 ASCII 都合法。控制字符用 String.fromCharCode 构造，避免源码内嵌生字节。
const NUL = String.fromCharCode(0);
const DEL = String.fromCharCode(0x7f);

describe("isValidUserAgentHeader", () => {
  it("treats empty / whitespace-only as valid (unset)", () => {
    expect(isValidUserAgentHeader("")).toBe(true);
    expect(isValidUserAgentHeader("   ")).toBe(true);
  });

  it("accepts visible ASCII (trimmed)", () => {
    expect(isValidUserAgentHeader("claude-cli/2.1.161")).toBe(true);
    expect(isValidUserAgentHeader("  claude-cli/2.1.161  ")).toBe(true);
  });

  it("accepts non-ASCII — matches backend HeaderValue byte rule", () => {
    expect(isValidUserAgentHeader("claude-cli/1.0 中文")).toBe(true);
  });

  it("accepts internal tab", () => {
    expect(isValidUserAgentHeader("claude\tcli")).toBe(true);
  });

  it("rejects control characters (newline / null / DEL)", () => {
    expect(isValidUserAgentHeader("claude\ncli")).toBe(false);
    expect(isValidUserAgentHeader(`claude${NUL}cli`)).toBe(false);
    expect(isValidUserAgentHeader(`claude${DEL}cli`)).toBe(false);
  });
});
