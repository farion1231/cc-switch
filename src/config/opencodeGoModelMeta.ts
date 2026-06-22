/**
 * OpenCode Go 模型元数据辅助：用于在「获取模型」时把上游 /v1/models 返回的模型
 * 富化为带正确上下文窗口的 Codex 模型目录条目，并排除仅 /messages 通路的模型。
 *
 * 这样当 OpenCode Go 上新增模型时，用户点一次「获取模型」即可自动拉取并套用上下文窗口，
 * 无需手动逐个填写。
 */
import type { CodexCatalogModel } from "../types";

/** OpenCode Go 的标准 base_url。 */
export const OPENCODE_GO_BASE_URL = "https://opencode.ai/zen/go/v1";

/** 判断某个 base_url 是否指向 OpenCode Go（容忍有无 /v1 与大小写）。 */
export function isOpencodeGoBaseUrl(baseUrl?: string | null): boolean {
  return !!baseUrl && baseUrl.toLowerCase().includes("opencode.ai/zen/go");
}

/**
 * 仅在 Anthropic /messages 通路可用、无法走 Codex 的 openai_chat 通路的模型。
 * 这些模型会从 chat 目录中排除（参见 docs：Go 的部分模型仅 /messages）。
 */
const MESSAGES_ONLY_MODELS = new Set<string>(["qwen3.7-max"]);

/**
 * 上下文窗口按模型 id 的有序前缀规则匹配；新加入的同系模型会自动继承同族窗口。
 * 数据依据 models.dev 的 opencode-go provider 与实测。
 */
const CONTEXT_RULES: Array<[RegExp, number]> = [
  [/^deepseek-v4/, 1_000_000],
  [/^glm-5\.2/, 1_000_000],
  [/^glm-5(\.1)?$/, 200_000],
  [/^glm-/, 200_000],
  [/^kimi-/, 262_144],
  [/^mimo-v2\.5-pro/, 1_048_576],
  [/^mimo-v2-pro/, 1_048_576],
  [/^mimo-v2\.5/, 1_000_000],
  [/^mimo-/, 262_144],
  [/^qwen3\.[67]-plus/, 1_000_000],
  [/^qwen3\.5-plus/, 262_144],
  [/^qwen/, 262_144],
  [/^minimax-m3/, 512_000],
  [/^minimax-/, 204_800],
];

/** 未知模型的兜底上下文窗口（保守值）。 */
const DEFAULT_CONTEXT_WINDOW = 200_000;

function contextWindowFor(id: string): number {
  for (const [pattern, ctx] of CONTEXT_RULES) {
    if (pattern.test(id)) return ctx;
  }
  return DEFAULT_CONTEXT_WINDOW;
}

/** 已知模型的友好显示名；未知模型回退到「按 - 分词 + 首字母大写」。 */
const DISPLAY_NAME_OVERRIDES: Record<string, string> = {
  "deepseek-v4-flash": "DeepSeek V4 Flash",
  "deepseek-v4-pro": "DeepSeek V4 Pro",
  "glm-5.2": "GLM-5.2",
  "glm-5.1": "GLM-5.1",
  "glm-5": "GLM-5",
  "kimi-k2.7-code": "Kimi K2.7 Code",
  "kimi-k2.6": "Kimi K2.6",
  "kimi-k2.5": "Kimi K2.5",
  "mimo-v2.5-pro": "MiMo V2.5 Pro",
  "mimo-v2.5": "MiMo V2.5",
  "mimo-v2-pro": "MiMo V2 Pro",
  "mimo-v2-omni": "MiMo V2 Omni",
  "qwen3.7-plus": "Qwen3.7 Plus",
  "qwen3.6-plus": "Qwen3.6 Plus",
  "qwen3.5-plus": "Qwen3.5 Plus",
  "minimax-m3": "MiniMax M3",
  "minimax-m2.7": "MiniMax M2.7",
  "minimax-m2.5": "MiniMax M2.5",
};

function displayNameFor(id: string): string {
  const override = DISPLAY_NAME_OVERRIDES[id];
  if (override) return override;
  return id
    .split("-")
    .map((part) => (part ? part[0].toUpperCase() + part.slice(1) : part))
    .join(" ");
}

/**
 * 把 /v1/models 返回的模型（结构上只需 `{ id }`）富化为 Codex 目录条目，
 * 排除仅 /messages 的模型，并按族套用上下文窗口。
 */
export function enrichOpencodeGoModels(
  models: ReadonlyArray<{ id: string }>,
): CodexCatalogModel[] {
  return models
    .map((m) => m.id)
    .filter((id) => !!id && !MESSAGES_ONLY_MODELS.has(id))
    .map((id) => ({
      model: id,
      displayName: displayNameFor(id),
      contextWindow: contextWindowFor(id),
    }));
}
