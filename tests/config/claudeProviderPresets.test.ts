import { describe, expect, it } from "vitest";
import { providerPresets } from "@/config/claudeProviderPresets";
import { applyTemplateValues } from "@/utils/providerConfigUtils";

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

  it("should use matching context and auto-compact placeholders", () => {
    const env = (kimiForCoding!.settingsConfig as any).env;
    expect(env).toHaveProperty(
      "CLAUDE_CODE_MAX_CONTEXT_TOKENS",
      "${CLAUDE_CODE_MAX_CONTEXT_TOKENS}",
    );
    expect(env).toHaveProperty(
      "CLAUDE_CODE_AUTO_COMPACT_WINDOW",
      "${CLAUDE_CODE_AUTO_COMPACT_WINDOW}",
    );
  });

  it("should expose matching 256K context and auto-compact values", () => {
    const values = kimiForCoding!.templateValues as any;
    expect(values?.CLAUDE_CODE_MAX_CONTEXT_TOKENS).toMatchObject({
      defaultValue: "262144",
      editorValue: "262144",
    });
    expect(values?.CLAUDE_CODE_AUTO_COMPACT_WINDOW).toMatchObject({
      defaultValue: "262144",
      editorValue: "262144",
      label: "Auto Compact Window",
    });
  });

  it("should resolve both Kimi context placeholders", () => {
    const config = applyTemplateValues(
      kimiForCoding!.settingsConfig,
      kimiForCoding!.templateValues,
    ) as any;
    expect(config.env.CLAUDE_CODE_MAX_CONTEXT_TOKENS).toBe("262144");
    expect(config.env.CLAUDE_CODE_AUTO_COMPACT_WINDOW).toBe("262144");
  });
});

describe("Codex Provider Preset", () => {
  const codex = providerPresets.find((p) => p.name === "Codex");

  it("should include the Codex preset", () => {
    expect(codex).toBeDefined();
  });

  it("should override Claude Code's 200K fallback for GPT models", () => {
    const env = (codex!.settingsConfig as any).env;
    expect(env).toHaveProperty(
      "CLAUDE_CODE_MAX_CONTEXT_TOKENS",
      "${CLAUDE_CODE_MAX_CONTEXT_TOKENS}",
    );
    expect(env).toHaveProperty(
      "CLAUDE_CODE_AUTO_COMPACT_WINDOW",
      "${CLAUDE_CODE_AUTO_COMPACT_WINDOW}",
    );
  });

  it("should expose the Codex-catalog 372K window for both context knobs", () => {
    const values = codex!.templateValues as any;
    expect(values?.CLAUDE_CODE_MAX_CONTEXT_TOKENS).toMatchObject({
      defaultValue: "372000",
      editorValue: "372000",
    });
    expect(values?.CLAUDE_CODE_AUTO_COMPACT_WINDOW).toMatchObject({
      defaultValue: "372000",
      editorValue: "372000",
    });
  });

  it("should resolve both context placeholders into Claude Code env values", () => {
    const config = applyTemplateValues(
      codex!.settingsConfig,
      codex!.templateValues,
    ) as any;
    expect(config.env.CLAUDE_CODE_MAX_CONTEXT_TOKENS).toBe("372000");
    expect(config.env.CLAUDE_CODE_AUTO_COMPACT_WINDOW).toBe("372000");
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
