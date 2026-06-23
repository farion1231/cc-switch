import { describe, expect, it } from "vitest";
import {
  buildLocalProxyRequestOverrides,
  isValidHttpHeaderValue,
  parseHeaderOverrideJson,
  parseRequestOverrideJson,
} from "@/lib/requestOverrides";

describe("requestOverrides", () => {
  it("treats empty JSON fields as unset", () => {
    expect(buildLocalProxyRequestOverrides("", "   ")).toEqual({});
  });

  it("parses header and body override objects", () => {
    expect(
      buildLocalProxyRequestOverrides(
        '{ "X-Test": "ok" }',
        '{ "temperature": 0.2 }',
      ),
    ).toEqual({
      overrides: {
        headers: { "X-Test": "ok" },
        body: { temperature: 0.2 },
      },
    });
  });

  it("rejects non-object body overrides", () => {
    expect(parseRequestOverrideJson("[]").error).toBeTruthy();
  });

  it("rejects non-string header values", () => {
    expect(parseHeaderOverrideJson('{ "X-Test": 1 }').error).toBeTruthy();
  });

  it("matches backend header value control-character rule", () => {
    expect(isValidHttpHeaderValue("hello\tworld")).toBe(true);
    expect(isValidHttpHeaderValue("hello\nworld")).toBe(false);
  });
});
