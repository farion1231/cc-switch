import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useCommonConfigSnippet } from "@/components/providers/forms/hooks/useCommonConfigSnippet";

const getCommonConfigSnippetMock = vi.fn();
const setCommonConfigSnippetMock = vi.fn();

vi.mock("@/lib/api", () => ({
  configApi: {
    getCommonConfigSnippet: (...args: unknown[]) =>
      getCommonConfigSnippetMock(...args),
    setCommonConfigSnippet: (...args: unknown[]) =>
      setCommonConfigSnippetMock(...args),
    extractCommonConfigSnippet: vi.fn().mockResolvedValue(""),
  },
}));

describe("useCommonConfigSnippet", () => {
  beforeEach(() => {
    getCommonConfigSnippetMock.mockResolvedValue(
      JSON.stringify({ includeCoAuthoredBy: false }),
    );
    setCommonConfigSnippetMock.mockResolvedValue(undefined);
  });

  it("settingsConfig 多次相同变化不触发冗余 setUseCommonConfig", async () => {
    const onConfigChange = vi.fn();

    // 编辑模式 + initialEnabled: false → 不会自动合并 snippet
    const { result, rerender } = renderHook(
      ({ config }: { config: string }) =>
        useCommonConfigSnippet({
          settingsConfig: config,
          onConfigChange,
          initialData: { settingsConfig: { env: { ANTHROPIC_MODEL: "test" } } },
          initialEnabled: false,
        }),
      {
        initialProps: {
          config: JSON.stringify({
            env: { ANTHROPIC_MODEL: "test" },
          }),
        },
      },
    );

    await waitFor(() => expect(result.current.isLoading).toBe(false));

    // 编辑模式 + initialEnabled: false → useCommonConfig 应为 false
    await waitFor(() =>
      expect(result.current.useCommonConfig).toBe(false),
    );

    // 用内容不同但都不含 snippet 的 settingsConfig 多次 rerender
    // 应不会触发振荡（无限循环 setState → rerender → setState）
    for (let i = 0; i < 5; i++) {
      rerender({
        config: JSON.stringify({
          env: { ANTHROPIC_MODEL: "test", extra: `val-${i}` },
        }),
      });
    }

    // onConfigChange 不应因通用配置检查而被调用
    expect(onConfigChange).not.toHaveBeenCalled();
    expect(result.current.useCommonConfig).toBe(false);
  });

  it("settingsConfig 变化导致通用配置包含状态改变时更新 useCommonConfig", async () => {
    const onConfigChange = vi.fn();

    // 初始配置不含通用配置片段
    const configWithoutSnippet = JSON.stringify({
      env: { ANTHROPIC_MODEL: "test" },
    });

    const { result, rerender } = renderHook(
      ({ config }: { config: string }) =>
        useCommonConfigSnippet({
          settingsConfig: config,
          onConfigChange,
          initialData: { settingsConfig: { env: { ANTHROPIC_MODEL: "test" } } },
          initialEnabled: false,
        }),
      {
        initialProps: { config: configWithoutSnippet },
      },
    );

    await waitFor(() => expect(result.current.isLoading).toBe(false));
    await waitFor(() =>
      expect(result.current.useCommonConfig).toBe(false),
    );

    // 配置现在包含通用配置片段
    const configWithSnippet = JSON.stringify({
      env: { ANTHROPIC_MODEL: "test" },
      includeCoAuthoredBy: false,
    });

    rerender({ config: configWithSnippet });

    await waitFor(() =>
      expect(result.current.useCommonConfig).toBe(true),
    );
  });

  it("isUpdatingFromCommonConfig 期间跳过状态检查", async () => {
    const onConfigChange = vi.fn();

    const { result } = renderHook(() =>
      useCommonConfigSnippet({
        settingsConfig: JSON.stringify({ env: { ANTHROPIC_MODEL: "test" } }),
        onConfigChange,
        initialData: { settingsConfig: { env: { ANTHROPIC_MODEL: "test" } } },
        initialEnabled: false,
      }),
    );

    await waitFor(() => expect(result.current.isLoading).toBe(false));

    // 手动触发 toggle（内部会设置 isUpdatingFromCommonConfig）
    act(() => {
      result.current.handleCommonConfigToggle(true);
    });

    // onConfigChange 应被调用了一次（由 toggle 发起）
    expect(onConfigChange).toHaveBeenCalled();
  });
});
