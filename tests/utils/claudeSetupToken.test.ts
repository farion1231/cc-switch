import { describe, expect, it } from "vitest";
import { terminalIsIsolated } from "@/components/providers/OpenTerminalDialog";
import type { Provider } from "@/types";
import {
  clearClaudeSetupTokenInConfig,
  getClaudeSetupTokenFromConfig,
  hasClaudeSetupToken,
  looksLikeClaudeSetupToken,
  setClaudeSetupTokenInConfig,
} from "@/utils/claudeSetupToken";

describe("claudeSetupToken utils", () => {
  it("reads and detects CLAUDE_CODE_OAUTH_TOKEN from object or JSON string", () => {
    expect(getClaudeSetupTokenFromConfig(undefined)).toBe("");
    expect(hasClaudeSetupToken({ env: {} })).toBe(false);
    expect(
      hasClaudeSetupToken({ env: { CLAUDE_CODE_OAUTH_TOKEN: "   " } }),
    ).toBe(false);
    expect(
      getClaudeSetupTokenFromConfig({
        env: { CLAUDE_CODE_OAUTH_TOKEN: "sk-ant-oat01-demo" },
      }),
    ).toBe("sk-ant-oat01-demo");
    expect(
      hasClaudeSetupToken(
        JSON.stringify({
          env: { CLAUDE_CODE_OAUTH_TOKEN: "sk-ant-oat01-demo" },
        }),
      ),
    ).toBe(true);
  });

  it("sets and clears CLAUDE_CODE_OAUTH_TOKEN while preserving unrelated env keys", () => {
    const base = JSON.stringify({ env: { CUSTOM_FLAG: "1" }, model: "opus" });
    const withToken = setClaudeSetupTokenInConfig(
      base,
      "  sk-ant-oat01-demo  ",
    );
    expect(JSON.parse(withToken)).toEqual({
      env: { CUSTOM_FLAG: "1", CLAUDE_CODE_OAUTH_TOKEN: "sk-ant-oat01-demo" },
      model: "opus",
    });

    const cleared = clearClaudeSetupTokenInConfig(withToken);
    expect(JSON.parse(cleared)).toEqual({
      env: { CUSTOM_FLAG: "1" },
      model: "opus",
    });
  });

  it("recognizes standard setup-token prefix", () => {
    expect(looksLikeClaudeSetupToken("sk-ant-oat01-abc")).toBe(true);
    expect(looksLikeClaudeSetupToken("sk-ant-api03-abc")).toBe(false);
  });

  it("treats Claude official setup-token cards as isolated for Open Terminal", () => {
    const baseOfficial: Provider = {
      id: "claude-official",
      name: "Claude Official",
      category: "official",
      settingsConfig: { env: {} },
    };
    expect(terminalIsIsolated(baseOfficial, "claude")).toBe(false);

    const setupTokenOfficial: Provider = {
      ...baseOfficial,
      settingsConfig: {
        env: { CLAUDE_CODE_OAUTH_TOKEN: "sk-ant-oat01-demo" },
      },
    };
    expect(terminalIsIsolated(setupTokenOfficial, "claude")).toBe(true);
  });
});
