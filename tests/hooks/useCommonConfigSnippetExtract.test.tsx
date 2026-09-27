import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useCommonConfigSnippet } from "@/components/providers/forms/hooks/useCommonConfigSnippet";

const getCommonConfigSnippetMock = vi.fn();
const setCommonConfigSnippetMock = vi.fn();
const extractCommonConfigSnippetMock = vi.fn();

vi.mock("@/lib/api", () => ({
  configApi: {
    getCommonConfigSnippet: (...args: unknown[]) =>
      getCommonConfigSnippetMock(...args),
    setCommonConfigSnippet: (...args: unknown[]) =>
      setCommonConfigSnippetMock(...args),
    extractCommonConfigSnippet: (...args: unknown[]) =>
      extractCommonConfigSnippetMock(...args),
  },
}));

const SNIPPET = JSON.stringify({ includeCoAuthoredBy: false }, null, 2);

// 冲掉 hook 内部 setTimeout(0) 的标记重置与微任务队列
async function flushTimers() {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 10));
  });
}

describe("useCommonConfigSnippet extract source", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    getCommonConfigSnippetMock.mockResolvedValue(SNIPPET);
    setCommonConfigSnippetMock.mockResolvedValue(undefined);
  });

  it("extracts from the pristine saved config when the form was auto-merged", async () => {
    // #7712：保存副本里 includeCoAuthoredBy 与片段同键不同值 → 编辑模式初始化
    // 判定"未应用"并自动 merge，onConfigChange 收到覆盖后的内容（表单被污染）。
    // 提取必须从数据库里的原始副本出发，否则只能提回片段自身的回声。
    extractCommonConfigSnippetMock.mockResolvedValue(
      JSON.stringify({ includeCoAuthoredBy: true, hooks: { Stop: [] } }),
    );

    const savedSettings = {
      env: { ANTHROPIC_MODEL: "claude-3" },
      includeCoAuthoredBy: true,
      hooks: { Stop: [] },
    };
    const pristineConfig = JSON.stringify(savedSettings, null, 2);
    const onConfigChange = vi.fn();
    const { result, rerender } = renderHook(
      ({ config }: { config: string }) =>
        useCommonConfigSnippet({
          settingsConfig: config,
          onConfigChange,
          initialData: { settingsConfig: savedSettings },
          initialEnabled: true,
        }),
      { initialProps: { config: pristineConfig } },
    );

    await waitFor(() => expect(result.current.isLoading).toBe(false));
    await flushTimers();

    // 自动合并已把表单覆盖成片段值（includeCoAuthoredBy 回到 false）
    const pollutedConfig = onConfigChange.mock.calls.at(-1)?.[0] as string;
    expect(pollutedConfig).toBeDefined();
    expect(pollutedConfig).not.toBe(pristineConfig);
    rerender({ config: pollutedConfig });

    await act(async () => {
      await result.current.handleExtract();
    });

    // 提取载荷是未污染的已保存副本，而非被合并覆盖的表单内容
    expect(extractCommonConfigSnippetMock).toHaveBeenCalledTimes(1);
    const payload = extractCommonConfigSnippetMock.mock.calls[0][1] as {
      settingsConfig: string;
    };
    expect(payload.settingsConfig).toBe(pristineConfig);
  });

  it("falls back to the form content in create mode", async () => {
    extractCommonConfigSnippetMock.mockResolvedValue(
      JSON.stringify({ includeCoAuthoredBy: false }),
    );

    const formConfig = JSON.stringify({ env: { FOO: "1" } }, null, 2);
    const { result } = renderHook(() =>
      useCommonConfigSnippet({
        settingsConfig: formConfig,
        onConfigChange: vi.fn(),
      }),
    );

    await waitFor(() => expect(result.current.isLoading).toBe(false));
    await flushTimers();

    await act(async () => {
      await result.current.handleExtract();
    });

    expect(extractCommonConfigSnippetMock).toHaveBeenCalledTimes(1);
    const payload = extractCommonConfigSnippetMock.mock.calls[0][1] as {
      settingsConfig: string;
    };
    expect(payload.settingsConfig).toBe(formConfig);
  });
});

