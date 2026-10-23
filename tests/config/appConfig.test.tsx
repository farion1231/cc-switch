import { describe, expect, it } from "vitest";
import {
  isAdditiveAppId,
  SKILLS_APP_IDS,
  MCP_APP_IDS,
  isProxyAppId,
} from "@/config/appConfig";

describe("appConfig provider lifecycle", () => {
  it.each([
    "opencode",
    "openclaw",
    "hermes",
    "pi",
    "mcode",
    "kimi",
    "workbuddy",
  ])("classifies %s as additive", (appId) => {
    expect(isAdditiveAppId(appId)).toBe(true);
  });

  it.each(["claude", "claude-desktop", "codex", "gemini", "grokbuild"])(
    "does not classify %s as additive",
    (appId) => {
      expect(isAdditiveAppId(appId)).toBe(false);
    },
  );
});

// Antigravity shares management UI, but has independent configuration and no CCS proxy.
describe("Antigravity capabilities", () => {
  it("supports skills and MCP with direct provider switching", () => {
    expect(SKILLS_APP_IDS).toContain("agy");
    expect(MCP_APP_IDS).toContain("agy");
    expect(isAdditiveAppId("agy")).toBe(false);
    expect(isProxyAppId("agy")).toBe(false);
  });
});

// WorkBuddy manages providers, prompts, MCP (~/.workbuddy/mcp.json) and Skills
// (~/.workbuddy/skills) but has no local proxy.
describe("WorkBuddy capabilities", () => {
  it("is additive with MCP and Skills but without the local proxy", () => {
    expect(isAdditiveAppId("workbuddy")).toBe(true);
    expect(SKILLS_APP_IDS).toContain("workbuddy");
    expect((MCP_APP_IDS as string[]).includes("workbuddy")).toBe(true);
    expect(isProxyAppId("workbuddy")).toBe(false);
  });
});
