import { describe, expect, it } from "vitest";
import {
  apiKeyRefStatus,
  isMalformedApiKeyRef,
  isValidApiKeyVarName,
  parseApiKeyRef,
  type ApiKeyVar,
} from "@/lib/api/apiKeyVars";

const variable = (over: Partial<ApiKeyVar> = {}): ApiKeyVar => ({
  name: "MY_KEY",
  enabled: true,
  scriptDir: "/tmp/s",
  hasValue: true,
  maskedValue: "sk-1…abcd",
  state: { consecutiveFailures: 0, needsAttention: false },
  references: [],
  ...over,
});

describe("apiKeyVars helpers", () => {
  it("validates variable names like the backend", () => {
    expect(isValidApiKeyVarName("MY_KEY")).toBe(true);
    expect(isValidApiKeyVarName("_A1")).toBe(true);
    expect(isValidApiKeyVarName("my_key")).toBe(false);
    expect(isValidApiKeyVarName("1A")).toBe(false);
    expect(isValidApiKeyVarName("CC_SWITCH_X")).toBe(false);
    expect(isValidApiKeyVarName("A".repeat(65))).toBe(false);
  });

  it("parses whole-field references only", () => {
    expect(parseApiKeyRef("{{MY_KEY}}")).toBe("MY_KEY");
    expect(parseApiKeyRef("  {{MY_KEY}} ")).toBe("MY_KEY");
    expect(parseApiKeyRef("Bearer {{MY_KEY}}")).toBeNull();
    expect(parseApiKeyRef("{{ my }}")).toBeNull();
    expect(isMalformedApiKeyRef("Bearer {{MY_KEY}}")).toBe(true);
    expect(isMalformedApiKeyRef("sk-plain")).toBe(false);
  });

  it("computes reference status from the variable list", () => {
    expect(apiKeyRefStatus("sk-plain", [])).toBeNull();
    expect(apiKeyRefStatus("{{MY_KEY}}", [])?.status).toBe("unknown");
    expect(
      apiKeyRefStatus("{{MY_KEY}}", [variable({ enabled: false })])?.status,
    ).toBe("disabled");
    expect(
      apiKeyRefStatus("{{MY_KEY}}", [variable({ hasValue: false })])?.status,
    ).toBe("noValue");
    expect(apiKeyRefStatus("{{MY_KEY}}", [variable()])?.status).toBe("ok");
    expect(apiKeyRefStatus("x {{MY_KEY}}", [variable()])?.status).toBe(
      "malformed",
    );
  });
});
