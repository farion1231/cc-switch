import { describe, expect, it } from "vitest";
import {
  getOpenClawTimeoutInputValue,
  getOpenClawToolsProfileSelectValue,
  getOpenClawUnsupportedProfile,
  OPENCLAW_UNSUPPORTED_PROFILE,
  parseOpenClawEnvEditorValue,
} from "@/components/openclaw/utils";

describe("OpenClaw utils", () => {
  it("parses nested env objects without stringifying them", () => {
    const env = parseOpenClawEnvEditorValue(`{
      "API_KEY": "secret",
      "vars": { "HTTP_PROXY": "http://127.0.0.1:8080" },
      "shellEnv": { "NODE_OPTIONS": "--max-old-space-size=4096" }
    }`);

    expect(env).toEqual({
      API_KEY: "secret",
      vars: { HTTP_PROXY: "http://127.0.0.1:8080" },
      shellEnv: { NODE_OPTIONS: "--max-old-space-size=4096" },
    });
  });

  it("rejects non-object env payloads", () => {
    expect(() => parseOpenClawEnvEditorValue(`["not", "an object"]`)).toThrow(
      "OPENCLAW_ENV_OBJECT_REQUIRED",
    );
  });

  it("flags unsupported tools profiles without silently normalizing them", () => {
    expect(getOpenClawToolsProfileSelectValue("default")).toBe(
      OPENCLAW_UNSUPPORTED_PROFILE,
    );
    expect(getOpenClawUnsupportedProfile("default")).toBe("default");
    expect(getOpenClawUnsupportedProfile("coding")).toBeNull();
  });

  it("prefers timeoutSeconds and falls back to legacy timeout", () => {
    expect(
      getOpenClawTimeoutInputValue({ timeoutSeconds: 120, timeout: 30 }),
    ).toBe("120");
    expect(getOpenClawTimeoutInputValue({ timeout: 45 })).toBe("45");
    expect(getOpenClawTimeoutInputValue({})).toBe("");
  });
});
