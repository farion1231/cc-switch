import { describe, expect, it } from "vitest";
import { parse as parseToml } from "smol-toml";
import { providerPresets } from "@/config/claudeProviderPresets";
import { codexProviderPresets } from "@/config/codexProviderPresets";

// 预设只描述上游：地址、鉴权、模型和上游兼容选项。插件、署名、推理档位这类
// 个人偏好不属于供应商，写进预设会在切换时覆盖用户自己的设置（E-FlowCode 曾把
// enabledPlugins 整张覆盖掉用户的插件表）。

describe("Claude presets carry no personal preferences", () => {
  // Claude Code settings.json 的顶层键除 env 外都是用户偏好（permissions、hooks、
  // enabledPlugins、includeCoAuthoredBy……）；顶层 apiKey Claude Code 不读。
  it.each(providerPresets.map((preset) => [preset.name, preset] as const))(
    "%s only sets env",
    (_name, preset) => {
      expect(Object.keys(preset.settingsConfig as object)).toEqual(["env"]);
    },
  );
});

describe("Codex presets carry no personal preferences", () => {
  // 顶层只允许模型、协议相关的键；personality、approval_policy、sandbox_mode、
  // notify、tui 这类偏好一律不进预设。model_context_window /
  // model_auto_compact_token_limit 是跟上游走的窗口键，预留给顶层。
  const allowedTopLevelKeys = new Set([
    "model_provider",
    "model",
    "model_reasoning_effort",
    "disable_response_storage",
    "review_model",
    "model_verbosity",
    "model_context_window",
    "model_auto_compact_token_limit",
    "model_providers",
  ]);

  it.each(codexProviderPresets.map((preset) => [preset.name, preset] as const))(
    "%s only sets upstream keys at the top level",
    (_name, preset) => {
      const config = parseToml(preset.config ?? "");
      const unexpected = Object.keys(config).filter(
        (key) => !allowedTopLevelKeys.has(key),
      );
      expect(unexpected).toEqual([]);
    },
  );
});
