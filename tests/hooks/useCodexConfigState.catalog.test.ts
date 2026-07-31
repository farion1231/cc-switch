import { renderHook } from "@testing-library/react";
import { describe, it, expect } from "vitest";
import { useCodexConfigState } from "@/components/providers/forms/hooks/useCodexConfigState";

// 回归：编辑已存在的原生 Responses 供应商时，读回 modelCatalog 必须保留隐藏字段
// 各类隐藏 capability，否则保存会把它们剥掉，导致生成的 Codex catalog
// 丢官方工具、reasoning、截断与版本能力。
//
// 注意：initialData 必须是稳定引用（hook 的 init effect 依赖 [initialData]）。
// 写成内联字面量会每次 re-render 产生新引用 → effect 反复 setState → 死循环 OOM。
describe("useCodexConfigState catalog load", () => {
  it("preserves native-profile hidden fields (camelCase, DB SSOT)", () => {
    const initialData = {
      settingsConfig: {
        auth: { OPENAI_API_KEY: "sk-x" },
        config: "",
        modelCatalog: {
          models: [
            {
              model: "MiniMax-M3",
              displayName: "MiniMax-M3",
              contextWindow: 1000000,
              supportsParallelToolCalls: true,
              inputModalities: ["text", "image"],
              baseInstructions: "You are Codex, based on MiniMax-M3.",
              applyPatchToolType: "freeform",
              webSearchToolType: "text",
              supportsSearchTool: true,
              supportVerbosity: true,
              defaultVerbosity: "low",
              supportedReasoningLevels: [
                { effort: "low", description: "Light reasoning" },
                { effort: "high", description: "Deep reasoning" },
              ],
              defaultReasoningLevel: "high",
              truncationPolicy: { mode: "tokens", limit: 10000 },
              multiAgentVersion: "v2",
              minimalClientVersion: "0.144.0",
            },
          ],
        },
      },
    };

    const { result } = renderHook(() => useCodexConfigState({ initialData }));

    expect(result.current.codexCatalogModels).toEqual([
      {
        model: "MiniMax-M3",
        displayName: "MiniMax-M3",
        contextWindow: 1000000,
        supportsParallelToolCalls: true,
        inputModalities: ["text", "image"],
        baseInstructions: "You are Codex, based on MiniMax-M3.",
        applyPatchToolType: "freeform",
        webSearchToolType: "text",
        supportsSearchTool: true,
        supportVerbosity: true,
        defaultVerbosity: "low",
        supportedReasoningLevels: [
          { effort: "low", description: "Light reasoning" },
          { effort: "high", description: "Deep reasoning" },
        ],
        defaultReasoningLevel: "high",
        truncationPolicy: { mode: "tokens", limit: 10000 },
        multiAgentVersion: "v2",
        minimalClientVersion: "0.144.0",
      },
    ]);
  });

  it("maps snake_case hidden fields (live reverse-parse fallback) to camelCase", () => {
    const initialData = {
      settingsConfig: {
        auth: {},
        config: "",
        modelCatalog: {
          models: [
            {
              model: "mimo-v2.5-pro",
              display_name: "MiMo V2.5 Pro",
              context_window: 262144,
              supports_parallel_tool_calls: false,
              input_modalities: ["text"],
              base_instructions: "You are MiMo, developed by Xiaomi.",
              apply_patch_tool_type: "freeform",
              web_search_tool_type: "text_and_image",
              supports_search_tool: false,
              support_verbosity: true,
              default_verbosity: "low",
              supported_reasoning_levels: [
                { effort: "low", description: "Light reasoning" },
                { effort: "max", description: "Maximum reasoning" },
              ],
              default_reasoning_level: "max",
              truncation_policy: { mode: "bytes", limit: 8192 },
              multi_agent_version: "v2",
              minimal_client_version: "0.144.0",
            },
          ],
        },
      },
    };

    const { result } = renderHook(() => useCodexConfigState({ initialData }));

    expect(result.current.codexCatalogModels).toEqual([
      {
        model: "mimo-v2.5-pro",
        displayName: "MiMo V2.5 Pro",
        contextWindow: 262144,
        supportsParallelToolCalls: false,
        inputModalities: ["text"],
        baseInstructions: "You are MiMo, developed by Xiaomi.",
        applyPatchToolType: "freeform",
        webSearchToolType: "text_and_image",
        supportsSearchTool: false,
        supportVerbosity: true,
        defaultVerbosity: "low",
        supportedReasoningLevels: [
          { effort: "low", description: "Light reasoning" },
          { effort: "max", description: "Maximum reasoning" },
        ],
        defaultReasoningLevel: "max",
        truncationPolicy: { mode: "bytes", limit: 8192 },
        multiAgentVersion: "v2",
        minimalClientVersion: "0.144.0",
      },
    ]);
  });

  it("normalizes reasoning levels and rejects malformed structured capabilities", () => {
    const initialData = {
      settingsConfig: {
        auth: {},
        config: "",
        modelCatalog: {
          models: [
            {
              model: "deepseek-v4-flash",
              applyPatchToolType: "function",
              webSearchToolType: "video",
              supportsSearchTool: "yes",
              supportedReasoningLevels: [
                { effort: " low ", description: " Light reasoning " },
                { effort: "", description: "Missing effort" },
                { effort: "max", description: "   " },
                null,
              ],
              defaultReasoningLevel: 42,
              truncationPolicy: { mode: "tokens", limit: 0 },
              multiAgentVersion: {},
              minimalClientVersion: false,
            },
          ],
        },
      },
    };

    const { result } = renderHook(() => useCodexConfigState({ initialData }));

    expect(result.current.codexCatalogModels).toEqual([
      {
        model: "deepseek-v4-flash",
        displayName: "",
        contextWindow: "",
        supportedReasoningLevels: [
          { effort: "low", description: "Light reasoning" },
        ],
      },
    ]);
  });
});
