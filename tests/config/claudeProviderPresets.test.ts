import { describe, expect, it } from "vitest";
import { providerPresets } from "@/config/claudeProviderPresets";
import type { Provider } from "@/types";
import { providerNeedsRouting } from "@/utils/providerCapabilities";

describe("Kimi For Coding Provider Preset", () => {
  const kimiForCoding = providerPresets.find(
    (p) => p.name === "Kimi For Coding",
  );

  it("should include Kimi For Coding preset", () => {
    expect(kimiForCoding).toBeDefined();
  });

  // CLAUDE_CODE_MAX_CONTEXT_TOKENS is ignored for claude-* model ids, so the
  // preset must route the endpoint's own alias for the context envs to bite
  it("should route the kimi-for-coding model id on every tier", () => {
    const env = (kimiForCoding!.settingsConfig as any).env;
    expect(env).toMatchObject({
      ANTHROPIC_MODEL: "kimi-for-coding",
      ANTHROPIC_DEFAULT_HAIKU_MODEL: "kimi-for-coding",
      ANTHROPIC_DEFAULT_SONNET_MODEL: "kimi-for-coding",
      ANTHROPIC_DEFAULT_OPUS_MODEL: "kimi-for-coding",
    });
  });

  // 预设直接钉值，不再暴露表单输入框；要调整的用户直接改 JSON 编辑框
  it("should pin the 256K context envs without exposing form fields", () => {
    const env = (kimiForCoding!.settingsConfig as any).env;
    expect(env.CLAUDE_CODE_MAX_CONTEXT_TOKENS).toBe("262144");
    expect(env.CLAUDE_CODE_AUTO_COMPACT_WINDOW).toBe("262144");
    expect(kimiForCoding!.templateValues).toBeUndefined();
  });
});

describe("Codex Provider Preset", () => {
  const codex = providerPresets.find((p) => p.name === "Codex");

  it("should include the Codex preset", () => {
    expect(codex).toBeDefined();
  });

  // 预设直接钉 Codex 目录的 372K 窗口（openai/codex#31860），不暴露表单输入框
  it("should pin the Codex-catalog 372K window without exposing form fields", () => {
    const env = (codex!.settingsConfig as any).env;
    expect(env.CLAUDE_CODE_MAX_CONTEXT_TOKENS).toBe("372000");
    expect(env.CLAUDE_CODE_AUTO_COMPACT_WINDOW).toBe("372000");
    expect(codex!.templateValues).toBeUndefined();
  });
});

describe("OpenCode Go Provider Preset", () => {
  const openCodeGo = providerPresets.find((p) => p.name === "OpenCode Go");

  it("should use the Go Anthropic compatibility endpoint with x-api-key auth", () => {
    expect(openCodeGo).toBeDefined();

    const env = (openCodeGo!.settingsConfig as any).env;
    expect(env).toMatchObject({
      ANTHROPIC_BASE_URL: "https://opencode.ai/zen/go",
      ANTHROPIC_API_KEY: "",
      ANTHROPIC_MODEL: "deepseek-v4-flash",
      ANTHROPIC_DEFAULT_HAIKU_MODEL: "deepseek-v4-flash",
      ANTHROPIC_DEFAULT_SONNET_MODEL: "deepseek-v4-flash",
      ANTHROPIC_DEFAULT_OPUS_MODEL: "deepseek-v4-flash",
    });
    // /messages 只认 x-api-key，Bearer 被网关静默忽略——勿换回 AUTH_TOKEN
    expect(env).not.toHaveProperty("ANTHROPIC_AUTH_TOKEN");
    // 原生 anthropic 直连的预设不写 apiFormat（缺省即免路由）
    expect(openCodeGo!.apiFormat).toBeUndefined();
    expect(openCodeGo!.apiKeyField).toBe("ANTHROPIC_API_KEY");
  });

  it("should not require local routing in Claude Code", () => {
    const provider: Provider = {
      id: "opencode-go",
      name: openCodeGo!.name,
      category: openCodeGo!.category,
      settingsConfig: openCodeGo!.settingsConfig as Record<string, any>,
      meta: {
        apiFormat: openCodeGo!.apiFormat,
        apiKeyField: openCodeGo!.apiKeyField,
      },
    };

    expect(providerNeedsRouting("claude", provider)).toBe(false);
  });
});

describe("AWS Bedrock Provider Presets", () => {
  const bedrockAksk = providerPresets.find(
    (p) => p.name === "AWS Bedrock (AKSK)",
  );

  it("should include AWS Bedrock (AKSK) preset", () => {
    expect(bedrockAksk).toBeDefined();
  });

  it("AKSK preset should have required AWS env variables", () => {
    const env = (bedrockAksk!.settingsConfig as any).env;
    expect(env).toHaveProperty("AWS_ACCESS_KEY_ID");
    expect(env).toHaveProperty("AWS_SECRET_ACCESS_KEY");
    expect(env).toHaveProperty("AWS_REGION");
    expect(env).toHaveProperty("CLAUDE_CODE_USE_BEDROCK", "1");
  });

  it("AKSK preset should have template values for AWS credentials", () => {
    expect(bedrockAksk!.templateValues).toBeDefined();
    expect(bedrockAksk!.templateValues!.AWS_ACCESS_KEY_ID).toBeDefined();
    expect(bedrockAksk!.templateValues!.AWS_SECRET_ACCESS_KEY).toBeDefined();
    expect(bedrockAksk!.templateValues!.AWS_REGION).toBeDefined();
    expect(bedrockAksk!.templateValues!.AWS_REGION.editorValue).toBe(
      "us-west-2",
    );
  });

  it("AKSK preset should have correct base URL template", () => {
    const env = (bedrockAksk!.settingsConfig as any).env;
    expect(env.ANTHROPIC_BASE_URL).toContain("bedrock-runtime");
    expect(env.ANTHROPIC_BASE_URL).toContain("${AWS_REGION}");
  });

  it("AKSK preset should have cloud_provider category", () => {
    expect(bedrockAksk!.category).toBe("cloud_provider");
  });

  it("AKSK preset should have Bedrock model as default", () => {
    const env = (bedrockAksk!.settingsConfig as any).env;
    expect(env.ANTHROPIC_MODEL).toContain("anthropic.claude");
  });

  const bedrockApiKey = providerPresets.find(
    (p) => p.name === "AWS Bedrock (API Key)",
  );

  it("should include AWS Bedrock (API Key) preset", () => {
    expect(bedrockApiKey).toBeDefined();
  });

  it("API Key preset should have apiKey field and AWS env variables", () => {
    const config = bedrockApiKey!.settingsConfig as any;
    expect(config).toHaveProperty("apiKey", "");
    expect(config.env).toHaveProperty("AWS_REGION");
    expect(config.env).toHaveProperty("CLAUDE_CODE_USE_BEDROCK", "1");
  });

  it("API Key preset should NOT have AKSK env variables", () => {
    const env = (bedrockApiKey!.settingsConfig as any).env;
    expect(env).not.toHaveProperty("AWS_ACCESS_KEY_ID");
    expect(env).not.toHaveProperty("AWS_SECRET_ACCESS_KEY");
  });

  it("API Key preset should have template values for region only", () => {
    expect(bedrockApiKey!.templateValues).toBeDefined();
    expect(bedrockApiKey!.templateValues!.AWS_REGION).toBeDefined();
    expect(bedrockApiKey!.templateValues!.AWS_REGION.editorValue).toBe(
      "us-west-2",
    );
  });

  it("API Key preset should have cloud_provider category", () => {
    expect(bedrockApiKey!.category).toBe("cloud_provider");
  });
});
