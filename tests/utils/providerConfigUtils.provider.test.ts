import { describe, expect, it } from "vitest";
import {
  extractClaudeBaseUrlFromConfig,
  extractCodexAuthEnvKey,
  getApiKeyFromConfig,
  setApiKeyInConfig,
  setClaudeBaseUrlInConfig,
} from "@/utils/providerConfigUtils";

describe("Provider config utils", () => {
  it("reads and writes Azure Foundry Claude settings", () => {
    const input = JSON.stringify(
      {
        env: {
          CLAUDE_CODE_USE_FOUNDRY: "1",
          ANTHROPIC_FOUNDRY_API_KEY: "",
          ANTHROPIC_FOUNDRY_RESOURCE: "demo-foundry",
        },
      },
      null,
      2,
    );

    const withUrl = setClaudeBaseUrlInConfig(
      input,
      "https://demo-foundry.services.ai.azure.com/anthropic",
    );
    expect(extractClaudeBaseUrlFromConfig(withUrl)).toBe(
      "https://demo-foundry.services.ai.azure.com/anthropic",
    );

    const withKey = setApiKeyInConfig(withUrl, "foundry-token", {
      appType: "claude",
    });
    expect(getApiKeyFromConfig(withKey, "claude")).toBe("foundry-token");
  });

  it("reads and writes Bedrock Claude settings through env", () => {
    const input = JSON.stringify(
      {
        env: {
          CLAUDE_CODE_USE_BEDROCK: "1",
          AWS_BEARER_TOKEN_BEDROCK: "",
          AWS_REGION: "us-east-1",
          ANTHROPIC_BASE_URL: "https://bedrock-runtime.us-east-1.amazonaws.com",
        },
      },
      null,
      2,
    );

    const output = setApiKeyInConfig(input, "bedrock-key", {
      appType: "claude",
    });

    expect(getApiKeyFromConfig(output, "claude")).toBe("bedrock-key");
  });

  it("uses codex env_key to read and write auth json", () => {
    const configToml = [
      'model_provider = "azure"',
      "",
      "[model_providers.azure]",
      'name = "Azure OpenAI"',
      'env_key = "AZURE_API_KEY"',
      'base_url = "https://example.openai.azure.com/openai"',
      'wire_api = "responses"',
      "",
    ].join("\n");

    const input = JSON.stringify(
      {
        auth: {
          AZURE_API_KEY: "old-key",
        },
        config: configToml,
      },
      null,
      2,
    );

    expect(extractCodexAuthEnvKey(configToml)).toBe("AZURE_API_KEY");
    expect(getApiKeyFromConfig(input, "codex")).toBe("old-key");

    const output = setApiKeyInConfig(input, "new-key", {
      appType: "codex",
    });

    expect(getApiKeyFromConfig(output, "codex")).toBe("new-key");
    expect(output).toContain('"AZURE_API_KEY": "new-key"');
    expect(output).not.toContain('"OPENAI_API_KEY"');
  });
});
