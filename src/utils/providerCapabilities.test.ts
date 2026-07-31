import { describe, it, expect } from "vitest";
import type { Provider } from "@/types";
import type { AppId } from "@/lib/api";
import {
  codexProviderPresets,
  type CodexProviderPreset,
} from "@/config/codexProviderPresets";
import { providerNeedsRouting } from "@/utils/providerCapabilities";
import {
  extractCodexModelName,
  extractCodexWireApi,
} from "@/utils/providerConfigUtils";

function mkProvider(overrides: Partial<Provider> = {}): Provider {
  return { id: "p1", name: "Test", settingsConfig: {}, ...overrides };
}

// wire_api 取自 config.toml；chat_completions 需转换（需路由），responses 直连。
const codexConfig = (wireApi: "chat_completions" | "responses") =>
  `model_provider = "custom"\n\n[model_providers.custom]\nname = "X"\nbase_url = "https://x.example/v1"\nwire_api = "${wireApi}"\n`;

function requireCodexPreset(name: string): CodexProviderPreset {
  const preset = codexProviderPresets.find((item) => item.name === name);
  if (!preset) throw new Error(`缺少 Codex 预设：${name}`);
  return preset;
}

function providerFromCodexPreset(preset: CodexProviderPreset): Provider {
  return mkProvider({
    id: `codex-preset-${preset.name}`,
    name: preset.name,
    category: preset.category,
    settingsConfig: {
      auth: preset.auth,
      config: preset.config,
      modelCatalog: { models: preset.modelCatalog ?? [] },
    },
    meta: {
      apiFormat: preset.apiFormat,
      providerType: preset.providerType,
      codexChatReasoning: preset.codexChatReasoning,
    },
  });
}

describe("providerNeedsRouting", () => {
  it("官方供应商一律不需要路由（即便 providerType 是 OAuth）", () => {
    const apps: AppId[] = ["claude", "codex", "claude-desktop"];
    for (const app of apps) {
      expect(
        providerNeedsRouting(
          app,
          mkProvider({
            category: "official",
            meta: { providerType: "xai_oauth" },
          }),
        ),
      ).toBe(false);
    }
  });

  describe("托管 OAuth：providerType 权威，与 apiFormat 无关（P2）", () => {
    it("Claude 下 xai_oauth 需要路由", () => {
      expect(
        providerNeedsRouting(
          "claude",
          mkProvider({
            meta: { providerType: "xai_oauth", apiFormat: "openai_responses" },
          }),
        ),
      ).toBe(true);
    });

    it("Claude 下 codex_oauth 即便 apiFormat 被改成 anthropic 仍需路由", () => {
      expect(
        providerNeedsRouting(
          "claude",
          mkProvider({
            meta: { providerType: "codex_oauth", apiFormat: "anthropic" },
          }),
        ),
      ).toBe(true);
    });

    it("Claude 下 codex_oauth 即便 apiFormat 缺省（旧数据）仍需路由", () => {
      expect(
        providerNeedsRouting(
          "claude",
          mkProvider({ meta: { providerType: "codex_oauth" } }),
        ),
      ).toBe(true);
    });

    it("Claude 下 github_copilot 需要路由", () => {
      expect(
        providerNeedsRouting(
          "claude",
          mkProvider({
            meta: { providerType: "github_copilot", apiFormat: "openai_chat" },
          }),
        ),
      ).toBe(true);
    });

    it("Codex 下 xai_oauth 需要路由（原生 Responses 也要注入 token）", () => {
      expect(
        providerNeedsRouting(
          "codex",
          mkProvider({
            meta: { providerType: "xai_oauth", apiFormat: "openai_responses" },
          }),
        ),
      ).toBe(true);
    });

    it("grokbuild 下 xai_oauth 需要路由", () => {
      expect(
        providerNeedsRouting(
          "grokbuild",
          mkProvider({
            meta: { providerType: "xai_oauth", apiFormat: "openai_responses" },
          }),
        ),
      ).toBe(true);
    });
  });

  describe("Claude 非 OAuth 按格式判定", () => {
    it("anthropic 原生直连不需要路由", () => {
      expect(
        providerNeedsRouting(
          "claude",
          mkProvider({ meta: { apiFormat: "anthropic" } }),
        ),
      ).toBe(false);
    });

    it("apiFormat 缺省视为原生直连，不需要路由", () => {
      expect(providerNeedsRouting("claude", mkProvider({ meta: {} }))).toBe(
        false,
      );
    });

    it("openai_chat 需要路由", () => {
      expect(
        providerNeedsRouting(
          "claude",
          mkProvider({ meta: { apiFormat: "openai_chat" } }),
        ),
      ).toBe(true);
    });

    it("openai_responses 需要路由", () => {
      expect(
        providerNeedsRouting(
          "claude",
          mkProvider({ meta: { apiFormat: "openai_responses" } }),
        ),
      ).toBe(true);
    });
  });

  describe("Codex 非 OAuth 按格式判定（Responses 直连）", () => {
    it("原生 Responses 不需要路由", () => {
      expect(
        providerNeedsRouting(
          "codex",
          mkProvider({ meta: { apiFormat: "openai_responses" } }),
        ),
      ).toBe(false);
    });

    it("Chat 格式需要路由", () => {
      expect(
        providerNeedsRouting(
          "codex",
          mkProvider({ meta: { apiFormat: "openai_chat" } }),
        ),
      ).toBe(true);
    });

    it("Anthropic 格式需要路由", () => {
      expect(
        providerNeedsRouting(
          "codex",
          mkProvider({ meta: { apiFormat: "anthropic" } }),
        ),
      ).toBe(true);
    });

    it("config 里 wire_api=chat_completions 需要路由", () => {
      expect(
        providerNeedsRouting(
          "codex",
          mkProvider({
            settingsConfig: { config: codexConfig("chat_completions") },
          }),
        ),
      ).toBe(true);
    });

    it("config 里 wire_api=responses 不需要路由", () => {
      expect(
        providerNeedsRouting(
          "codex",
          mkProvider({ settingsConfig: { config: codexConfig("responses") } }),
        ),
      ).toBe(false);
    });
  });

  describe("DeepSeek 真实 Codex 预设路由", () => {
    it("Flash 原生 Responses 直连，Pro 通过 OpenAI Chat 本地路由", () => {
      const flashPreset = requireCodexPreset("DeepSeek");
      const proPreset = requireCodexPreset("DeepSeek V4 Pro");

      // Codex 对两者都发送 Responses；apiFormat 决定是否转换成上游 Chat。
      expect(extractCodexWireApi(flashPreset.config)).toBe("responses");
      expect(extractCodexWireApi(proPreset.config)).toBe("responses");
      expect(flashPreset.apiFormat).toBe("openai_responses");
      expect(proPreset.apiFormat).toBe("openai_chat");

      expect(
        providerNeedsRouting("codex", providerFromCodexPreset(flashPreset)),
      ).toBe(false);
      expect(
        providerNeedsRouting("codex", providerFromCodexPreset(proPreset)),
      ).toBe(true);
    });

    it("Flash 与 Pro 的默认模型和模型目录彼此隔离", () => {
      const flashPreset = requireCodexPreset("DeepSeek");
      const proPreset = requireCodexPreset("DeepSeek V4 Pro");

      expect(extractCodexModelName(flashPreset.config)).toBe(
        "deepseek-v4-flash",
      );
      expect(extractCodexModelName(proPreset.config)).toBe("deepseek-v4-pro");
      expect(flashPreset.modelCatalog?.map((model) => model.model)).toEqual([
        "deepseek-v4-flash",
      ]);
      expect(proPreset.modelCatalog?.map((model) => model.model)).toEqual([
        "deepseek-v4-pro",
      ]);
    });
  });

  describe("Claude Desktop 路由判定", () => {
    it("proxy 模式需要路由", () => {
      expect(
        providerNeedsRouting(
          "claude-desktop",
          mkProvider({ meta: { claudeDesktopMode: "proxy" } }),
        ),
      ).toBe(true);
    });

    it.each(["github_copilot", "codex_oauth", "xai_oauth"])(
      "direct 模式的托管 OAuth %s 仍需要路由",
      (providerType) => {
        expect(
          providerNeedsRouting(
            "claude-desktop",
            mkProvider({
              meta: {
                providerType,
                claudeDesktopMode: "direct",
              },
            }),
          ),
        ).toBe(true);
      },
    );
  });
});
