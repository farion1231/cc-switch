import { describe, expect, it } from "vitest";
import {
  extractCodexBaseUrl,
  extractCodexModelName,
  setCodexBaseUrl,
  setCodexModelName,
} from "@/utils/providerConfigUtils";

const extractTomlSectionLines = (text: string, header: string): string[] => {
  const lines = text.split("\n");
  const headerIndex = lines.findIndex((line) => line.trim() === header);
  if (headerIndex === -1) return [];
  const start = headerIndex + 1;
  let end = lines.length;
  for (let i = start; i < lines.length; i += 1) {
    const trimmed = lines[i].trim();
    if (trimmed.startsWith("[") && trimmed.endsWith("]")) {
      end = i;
      break;
    }
  }
  return lines.slice(start, end);
};

describe("Codex TOML utils", () => {
  it("removes base_url line when set to empty", () => {
    const input = [
      'model_provider = "openai"',
      'base_url = "https://api.example.com/v1"',
      'model = "gpt-5-codex"',
      "",
    ].join("\n");

    const output = setCodexBaseUrl(input, "");

    expect(output).not.toMatch(/^\s*base_url\s*=/m);
    expect(extractCodexBaseUrl(output)).toBeUndefined();
    expect(extractCodexModelName(output)).toBe("gpt-5-codex");
  });

  it("removes model line when set to empty", () => {
    const input = [
      'model_provider = "openai"',
      'base_url = "https://api.example.com/v1"',
      'model = "gpt-5-codex"',
      "",
    ].join("\n");

    const output = setCodexModelName(input, "");

    expect(output).not.toMatch(/^\s*model\s*=/m);
    expect(extractCodexModelName(output)).toBeUndefined();
    expect(extractCodexBaseUrl(output)).toBe("https://api.example.com/v1");
  });

  it("updates existing values when non-empty", () => {
    const input = [
      'model_provider = "openai"',
      "base_url = 'https://old.example/v1'",
      'model = "old-model"',
      "",
    ].join("\n");

    const output1 = setCodexBaseUrl(input, " https://new.example/v1 \n");
    expect(extractCodexBaseUrl(output1)).toBe("https://new.example/v1");

    const output2 = setCodexModelName(output1, " new-model \n");
    expect(extractCodexModelName(output2)).toBe("new-model");
  });

  it("prefers active model provider base_url over [agents].base_url", () => {
    const input = [
      'model_provider = "custom"',
      'model = "gpt-5.4"',
      "",
      "[model_providers.custom]",
      'name = "custom"',
      'base_url = "https://provider.example/v1"',
      'wire_api = "responses"',
      'requires_openai_auth = true',
      "",
      "[agents]",
      'base_url = "http://wrong.example"',
      "max_depth = 4",
      "max_threads = 16",
      "",
    ].join("\n");

    expect(extractCodexBaseUrl(input)).toBe("https://provider.example/v1");
  });

  it("writes base_url into active model provider table and strips [agents].base_url", () => {
    const input = [
      'model_provider = "custom"',
      'model = "gpt-5.4"',
      "",
      "[model_providers.custom]",
      'name = "custom"',
      'wire_api = "responses"',
      'requires_openai_auth = true',
      "",
      "[agents]",
      'base_url = "http://legacy.example"',
      "max_depth = 4",
      "max_threads = 16",
      "",
      "[features]",
      "fast_mode = true",
      "multi_agent = true",
      "",
    ].join("\n");

    const output = setCodexBaseUrl(input, "https://new.example/v1");

    expect(extractCodexBaseUrl(output)).toBe("https://new.example/v1");

    const providerLines = extractTomlSectionLines(output, "[model_providers.custom]");
    expect(providerLines.join("\n")).toMatch(/^base_url\s*=\s*"https:\/\/new\.example\/v1"$/m);

    const agentLines = extractTomlSectionLines(output, "[agents]");
    expect(agentLines.join("\n")).not.toMatch(/^\s*base_url\s*=/m);
    expect(agentLines.join("\n")).toMatch(/^max_depth\s*=\s*4$/m);
    expect(agentLines.join("\n")).toMatch(/^max_threads\s*=\s*16$/m);
  });

  it("does not touch mcp_servers.* base_url when updating provider base_url", () => {
    const input = [
      'model_provider = "azure"',
      'model = "gpt-4"',
      "disable_response_storage = true",
      "",
      "[model_providers.azure]",
      'name = "Azure OpenAI"',
      'base_url = "https://old.azure/v1"',
      'wire_api = "responses"',
      "",
      "[mcp_servers.my_server]",
      'base_url = "http://localhost:8080"',
      "",
    ].join("\n");

    const output = setCodexBaseUrl(input, "https://new.azure/v1");

    expect(extractCodexBaseUrl(output)).toBe("https://new.azure/v1");
    const mcpLines = extractTomlSectionLines(output, "[mcp_servers.my_server]");
    expect(mcpLines.join("\n")).toMatch(/^base_url\s*=\s*"http:\/\/localhost:8080"$/m);
  });
});
