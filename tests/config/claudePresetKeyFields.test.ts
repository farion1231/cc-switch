import { describe, expect, it } from "vitest";
import { providerPresets } from "@/config/claudeProviderPresets";
import { isOAuthProviderType } from "@/config/constants";
import fields from "./claudeKeyFields.json";

// 切换时 CC Switch 只写关键字段（地址、凭据、模型名、协议）和供应商独有字段（上游
// 兼容开关、窗口值），预设里的其他键永远不会生效，只会显示在编辑器里误导用户。
// 清单是后端 `live::floor` 的镜像，由 Rust 测试保证两边一致。

const isKeyField = (key: string) =>
  fields.envPrefixes.some((prefix) => key.startsWith(prefix)) ||
  fields.protocolSelectors.includes(key) ||
  fields.envKeys.includes(key) ||
  (key.startsWith(fields.envSkipAuth.prefix) &&
    key.endsWith(fields.envSkipAuth.suffix));

const isExclusiveField = (key: string) => fields.exclusiveEnv.includes(key);

const presetEnv = (settingsConfig: unknown): Record<string, unknown> =>
  ((settingsConfig as { env?: Record<string, unknown> }).env ?? {}) as Record<
    string,
    unknown
  >;

const cases = providerPresets.map((preset) => [preset.name, preset] as const);

describe("Claude presets only carry provider-owned fields", () => {
  it.each(cases)("%s", (_name, preset) => {
    const stray = Object.keys(presetEnv(preset.settingsConfig)).filter(
      (key) => !isKeyField(key) && !isExclusiveField(key),
    );
    expect(stray).toEqual([]);
  });

  // `CLAUDE_CODE_USE_` 前缀下还有与供应商无关的功能开关，只有协议选择器随切换。
  it.each(cases)(
    "%s selects protocols from the known list",
    (_name, preset) => {
      const selectors = Object.keys(presetEnv(preset.settingsConfig)).filter(
        (key) => key.startsWith("CLAUDE_CODE_USE_"),
      );
      for (const selector of selectors) {
        expect(fields.protocolSelectors).toContain(selector);
      }
    },
  );
});

describe("Claude presets project an endpoint and a credential", () => {
  it.each(cases)("%s", (_name, preset) => {
    if (preset.category === "official") return;
    const env = presetEnv(preset.settingsConfig);
    const keys = Object.keys(env);
    // 托管 OAuth（Copilot、Codex、xAI）不带 Key：本地路由按请求注入登录凭据。
    if (isOAuthProviderType(preset.providerType)) {
      expect(keys).toContain("ANTHROPIC_BASE_URL");
      return;
    }
    if (env.CLAUDE_CODE_USE_BEDROCK) {
      expect(keys).toContain("AWS_REGION");
      expect(
        keys.some((key) =>
          [
            "AWS_BEARER_TOKEN_BEDROCK",
            "AWS_ACCESS_KEY_ID",
            "AWS_PROFILE",
          ].includes(key),
        ),
      ).toBe(true);
      return;
    }
    if (env.CLAUDE_CODE_USE_VERTEX) {
      expect(keys).toContain("CLOUD_ML_REGION");
      expect(keys).toContain("ANTHROPIC_VERTEX_PROJECT_ID");
      return;
    }
    expect(keys).toContain("ANTHROPIC_BASE_URL");
    expect(
      keys.some((key) =>
        ["ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_API_KEY"].includes(key),
      ),
    ).toBe(true);
  });
});
