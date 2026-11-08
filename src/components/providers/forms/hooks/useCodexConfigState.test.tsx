import { act, renderHook } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { useCodexConfigState } from "./useCodexConfigState";

// 已有供应商合法选择内置路由（后端 project/codex.rs 支持 BuiltIn 及 bedrock 覆盖表）：
// codexProviderId 是从 TOML 自动回显的，不代表用户新建了自定义表，保留名校验不能拦它。
// initialData 必须是稳定引用：hook 的初始化 effect 按 [initialData] 重跑（既有行为），
// 内联字面量会每渲染新建对象触发无限循环。
const initialData = {
  settingsConfig: {
    auth: { OPENAI_API_KEY: "" },
    config: `model = "gpt-5.4"\nmodel_provider = "amazon-bedrock"\n`,
  },
};

describe("useCodexConfigState provider id editing (issue #7856 review)", () => {
  it("does not flag an echoed selector as user-edited", () => {
    const { result } = renderHook(() => useCodexConfigState({ initialData }));
    expect(result.current.codexProviderId).toBe("amazon-bedrock");
    expect(result.current.codexProviderIdEdited).toBe(false);
  });

  it("flags the id once the user edits it", () => {
    const { result } = renderHook(() => useCodexConfigState({ initialData }));
    act(() => result.current.handleCodexProviderIdChange("my-bedrock"));
    expect(result.current.codexProviderId).toBe("my-bedrock");
    expect(result.current.codexProviderIdEdited).toBe(true);
  });

  it("clears the flag when an echo rewrites the field from the config", async () => {
    const { result } = renderHook(() => useCodexConfigState({ initialData }));
    act(() => result.current.handleCodexProviderIdChange("my-bedrock"));
    // 处理器用 setTimeout(0) 复位回显护栏：先让定时器跑完，下一次 config 变更才会回显。
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 0));
    });
    act(() =>
      result.current.handleCodexConfigChange(
        `model = "gpt-5.4"\nmodel_provider = "other-id"\n`,
      ),
    );
    expect(result.current.codexProviderId).toBe("other-id");
    expect(result.current.codexProviderIdEdited).toBe(false);
  });
});
