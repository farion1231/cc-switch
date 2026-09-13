import { describe, expect, it } from "vitest";
import {
  detectCodingPlanProvider,
  injectCodingPlanUsageScript,
} from "@/config/codingPlanProviders";

const CODEX_BASE_URL = "https://api.commandcode.ai/provider/v1";
const CLAUDE_BASE_URL = "https://api.commandcode.ai/provider";

describe("Command Code usage detection", () => {
  it("recognizes both provider base variants but not unrelated URLs", () => {
    expect(detectCodingPlanProvider(CLAUDE_BASE_URL)).toBe("command_code");
    expect(detectCodingPlanProvider(CODEX_BASE_URL)).toBe("command_code");
    expect(
      detectCodingPlanProvider("https://commandcode.ai/provider"),
    ).toBeNull();
  });

  it("injects the quota script for Claude Code and Codex", () => {
    const claude = injectCodingPlanUsageScript("claude", {
      settingsConfig: {
        env: { ANTHROPIC_BASE_URL: CLAUDE_BASE_URL },
      },
    }) as { meta?: { usage_script?: { codingPlanProvider?: string } } };
    const codex = injectCodingPlanUsageScript("codex", {
      settingsConfig: {
        config: `model_provider = "command_code"
[model_providers.command_code]
base_url = "${CODEX_BASE_URL}"`,
      },
    }) as { meta?: { usage_script?: { codingPlanProvider?: string } } };

    expect(claude.meta?.usage_script?.codingPlanProvider).toBe("command_code");
    expect(codex.meta?.usage_script?.codingPlanProvider).toBe("command_code");
  });

  it("does not extend Command Code into OpenCode", () => {
    const provider = injectCodingPlanUsageScript("opencode", {
      settingsConfig: {
        options: { baseURL: CODEX_BASE_URL },
      },
    }) as { meta?: { usage_script?: unknown } };

    expect(provider.meta?.usage_script).toBeUndefined();
  });

  it("does not overwrite an existing usage script", () => {
    const provider = injectCodingPlanUsageScript("codex", {
      settingsConfig: {
        config: `model_provider = "command_code"
[model_providers.command_code]
base_url = "${CODEX_BASE_URL}"`,
      },
      meta: {
        usage_script: { enabled: false, templateType: "custom" },
      },
    }) as { meta?: { usage_script?: unknown } };

    expect(provider.meta?.usage_script).toEqual({
      enabled: false,
      templateType: "custom",
    });
  });
});
