import { describe, expect, it } from "vitest";
import { providerPresets } from "@/config/claudeProviderPresets";

describe("MiniMax Provider Presets", () => {
  const minimaxCn = providerPresets.find((p) => p.name === "MiniMax");
  const minimaxEn = providerPresets.find((p) => p.name === "MiniMax en");
  const minimaxCnHighspeed = providerPresets.find(
    (p) => p.name === "MiniMax Highspeed",
  );
  const minimaxEnHighspeed = providerPresets.find(
    (p) => p.name === "MiniMax en Highspeed",
  );

  it("should include MiniMax (CN) preset", () => {
    expect(minimaxCn).toBeDefined();
  });

  it("should include MiniMax en preset", () => {
    expect(minimaxEn).toBeDefined();
  });

  it("should include MiniMax Highspeed (CN) preset", () => {
    expect(minimaxCnHighspeed).toBeDefined();
  });

  it("should include MiniMax en Highspeed preset", () => {
    expect(minimaxEnHighspeed).toBeDefined();
  });

  it("MiniMax (CN) should use MiniMax-M2.7 model", () => {
    const env = (minimaxCn!.settingsConfig as any).env;
    expect(env.ANTHROPIC_MODEL).toBe("MiniMax-M2.7");
    expect(env.ANTHROPIC_DEFAULT_SONNET_MODEL).toBe("MiniMax-M2.7");
    expect(env.ANTHROPIC_DEFAULT_OPUS_MODEL).toBe("MiniMax-M2.7");
    expect(env.ANTHROPIC_DEFAULT_HAIKU_MODEL).toBe("MiniMax-M2.7");
  });

  it("MiniMax en should use MiniMax-M2.7 model", () => {
    const env = (minimaxEn!.settingsConfig as any).env;
    expect(env.ANTHROPIC_MODEL).toBe("MiniMax-M2.7");
    expect(env.ANTHROPIC_DEFAULT_HAIKU_MODEL).toBe("MiniMax-M2.7");
  });

  it("MiniMax Highspeed (CN) should use MiniMax-M2.7-highspeed model", () => {
    const env = (minimaxCnHighspeed!.settingsConfig as any).env;
    expect(env.ANTHROPIC_MODEL).toBe("MiniMax-M2.7-highspeed");
    expect(env.ANTHROPIC_DEFAULT_SONNET_MODEL).toBe("MiniMax-M2.7-highspeed");
    expect(env.ANTHROPIC_DEFAULT_OPUS_MODEL).toBe("MiniMax-M2.7-highspeed");
    expect(env.ANTHROPIC_DEFAULT_HAIKU_MODEL).toBe("MiniMax-M2.7-highspeed");
  });

  it("MiniMax en Highspeed should use MiniMax-M2.7-highspeed model", () => {
    const env = (minimaxEnHighspeed!.settingsConfig as any).env;
    expect(env.ANTHROPIC_MODEL).toBe("MiniMax-M2.7-highspeed");
    expect(env.ANTHROPIC_DEFAULT_HAIKU_MODEL).toBe("MiniMax-M2.7-highspeed");
  });

  it("MiniMax Highspeed (CN) should use CN base URL", () => {
    const env = (minimaxCnHighspeed!.settingsConfig as any).env;
    expect(env.ANTHROPIC_BASE_URL).toBe("https://api.minimaxi.com/anthropic");
  });

  it("MiniMax en Highspeed should use international base URL", () => {
    const env = (minimaxEnHighspeed!.settingsConfig as any).env;
    expect(env.ANTHROPIC_BASE_URL).toBe("https://api.minimax.io/anthropic");
  });

  it("all MiniMax presets should have cn_official category", () => {
    expect(minimaxCn!.category).toBe("cn_official");
    expect(minimaxEn!.category).toBe("cn_official");
    expect(minimaxCnHighspeed!.category).toBe("cn_official");
    expect(minimaxEnHighspeed!.category).toBe("cn_official");
  });

  it("all MiniMax presets should be marked as partner", () => {
    expect(minimaxCn!.isPartner).toBe(true);
    expect(minimaxEn!.isPartner).toBe(true);
    expect(minimaxCnHighspeed!.isPartner).toBe(true);
    expect(minimaxEnHighspeed!.isPartner).toBe(true);
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
