import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useCommonConfigSnippet } from "@/components/providers/forms/hooks/useCommonConfigSnippet";
import { useCodexCommonConfig } from "@/components/providers/forms/hooks/useCodexCommonConfig";
import { useGeminiCommonConfig } from "@/components/providers/forms/hooks/useGeminiCommonConfig";

/**
 * 通用配置片段由 useInitialDataCommonConfig 在数据层合并进 initialData
 * （见 useInitialDataCommonConfig.test.tsx），表单收到的就是"合并后"的配置。
 *
 * 这里验证 UI 层剩下的那一半契约：勾选状态以 meta.commonConfigEnabled 为准，
 * 不被表单重置打回，也不被"片段推断不出信息"的情况静默改写。
 */

const apiMocks = vi.hoisted(() => ({
  getCommonConfigSnippet: vi.fn(),
  setCommonConfigSnippet: vi.fn(),
  extractCommonConfigSnippet: vi.fn(),
  updateTomlCommonConfigSnippet: vi.fn(),
}));

vi.mock("@/lib/api", () => ({
  configApi: {
    getCommonConfigSnippet: apiMocks.getCommonConfigSnippet,
    setCommonConfigSnippet: apiMocks.setCommonConfigSnippet,
    extractCommonConfigSnippet: apiMocks.extractCommonConfigSnippet,
    updateTomlCommonConfigSnippet: apiMocks.updateTomlCommonConfigSnippet,
  },
}));

const CLAUDE_SNIPPET = JSON.stringify({ plugin: "shared" });

const envStringToObj = (env: string): Record<string, string> =>
  Object.fromEntries(
    env
      .split("\n")
      .map((line) => line.trim())
      .filter(Boolean)
      .map((line) => {
        const index = line.indexOf("=");
        return [line.slice(0, index), line.slice(index + 1)];
      }),
  );
const envObjToString = (env: Record<string, unknown>): string =>
  Object.entries(env)
    .map(([key, value]) => `${key}=${String(value)}`)
    .join("\n");

describe("编辑供应商时的勾选状态", () => {
  beforeEach(() => {
    apiMocks.getCommonConfigSnippet.mockResolvedValue("");
    apiMocks.setCommonConfigSnippet.mockResolvedValue(undefined);
    apiMocks.extractCommonConfigSnippet.mockResolvedValue("");
    apiMocks.updateTomlCommonConfigSnippet.mockImplementation(
      async (configToml: string) => configToml,
    );
  });

  it("Claude：live 刷新换掉已合并的快照后仍保持勾选，且不再回写编辑器", async () => {
    apiMocks.getCommonConfigSnippet.mockResolvedValue(CLAUDE_SNIPPET);

    const onConfigChange = vi.fn();
    // 数据层已经合并过片段，表单拿到的是最终形态
    const storedData = {
      settingsConfig: { apiKey: "provider-key", plugin: "shared" },
    };
    const storedConfig = JSON.stringify(storedData.settingsConfig, null, 2);
    const liveData = {
      settingsConfig: { apiKey: "live-key", plugin: "shared" },
    };
    const liveConfig = JSON.stringify(liveData.settingsConfig, null, 2);

    const { result, rerender } = renderHook(
      ({
        settingsConfig,
        initialData,
      }: {
        settingsConfig: string;
        initialData: { settingsConfig: Record<string, unknown> };
      }) =>
        useCommonConfigSnippet({
          settingsConfig,
          onConfigChange,
          initialData,
          initialEnabled: true,
        }),
      {
        initialProps: { settingsConfig: storedConfig, initialData: storedData },
      },
    );

    await waitFor(() => expect(result.current.isLoading).toBe(false));
    await waitFor(() => expect(result.current.useCommonConfig).toBe(true));
    // 合并已在数据层完成，hook 不该再往编辑器写任何东西
    expect(onConfigChange).not.toHaveBeenCalled();

    // 模拟 EditProviderDialog 读到 live 配置后替换 initialData 并重置表单
    await act(async () => {
      rerender({ settingsConfig: storedConfig, initialData: liveData });
    });
    await act(async () => {
      rerender({ settingsConfig: liveConfig, initialData: liveData });
    });

    expect(result.current.useCommonConfig).toBe(true);
    expect(onConfigChange).not.toHaveBeenCalled();
  });

  it("Claude：用户手动删掉片段后取消勾选", async () => {
    apiMocks.getCommonConfigSnippet.mockResolvedValue(CLAUDE_SNIPPET);

    const onConfigChange = vi.fn();
    const initialData = {
      settingsConfig: { apiKey: "k", plugin: "shared" },
    };
    const mergedConfig = JSON.stringify(initialData.settingsConfig, null, 2);

    const { result, rerender } = renderHook(
      ({ settingsConfig }: { settingsConfig: string }) =>
        useCommonConfigSnippet({
          settingsConfig,
          onConfigChange,
          initialData,
          initialEnabled: true,
        }),
      { initialProps: { settingsConfig: mergedConfig } },
    );

    await waitFor(() => expect(result.current.isLoading).toBe(false));
    await waitFor(() => expect(result.current.useCommonConfig).toBe(true));

    await act(async () => {
      rerender({
        settingsConfig: JSON.stringify({ apiKey: "k" }, null, 2),
      });
    });
    expect(result.current.useCommonConfig).toBe(false);
  });

  it("Claude：用户取消勾选后，同一 initialData 的重渲染不会把勾选状态恢复", async () => {
    apiMocks.getCommonConfigSnippet.mockResolvedValue(CLAUDE_SNIPPET);

    const onConfigChange = vi.fn();
    const initialData = {
      settingsConfig: { apiKey: "k", plugin: "shared" },
    };
    const mergedConfig = JSON.stringify(initialData.settingsConfig, null, 2);

    const { result, rerender } = renderHook(
      ({ settingsConfig }: { settingsConfig: string }) =>
        useCommonConfigSnippet({
          settingsConfig,
          onConfigChange,
          initialData,
          initialEnabled: true,
        }),
      { initialProps: { settingsConfig: mergedConfig } },
    );

    await waitFor(() => expect(result.current.isLoading).toBe(false));
    await waitFor(() => expect(result.current.useCommonConfig).toBe(true));

    await act(async () => {
      result.current.handleCommonConfigToggle(false);
    });
    const stripped = onConfigChange.mock.calls.at(-1)?.[0] as string;
    expect(JSON.parse(stripped)).toEqual({ apiKey: "k" });

    // 父组件应用剥离结果后重渲染：勾选状态必须保持用户的选择
    await act(async () => {
      rerender({ settingsConfig: stripped });
    });
    expect(result.current.useCommonConfig).toBe(false);
  });

  it("Claude：片段推断不出信息时保持 flag，不静默改写成未勾选", async () => {
    // 后端没有存过片段，hook 落到内置默认片段，而快照里当然不含它
    apiMocks.getCommonConfigSnippet.mockResolvedValue("");

    const onConfigChange = vi.fn();
    const initialData = { settingsConfig: { apiKey: "k" } };

    const { result } = renderHook(() =>
      useCommonConfigSnippet({
        settingsConfig: JSON.stringify(initialData.settingsConfig, null, 2),
        onConfigChange,
        initialData,
        initialEnabled: true,
      }),
    );

    await waitFor(() => expect(result.current.isLoading).toBe(false));
    await act(async () => {});
    // 若这里被推断成 false，保存时会把 meta.commonConfigEnabled 从 true 改掉
    expect(result.current.useCommonConfig).toBe(true);
  });

  it("Claude：没有 flag 的旧供应商按内容推断", async () => {
    apiMocks.getCommonConfigSnippet.mockResolvedValue(CLAUDE_SNIPPET);

    const onConfigChange = vi.fn();
    const initialData = {
      settingsConfig: { apiKey: "k", plugin: "shared" },
    };

    const { result } = renderHook(() =>
      useCommonConfigSnippet({
        settingsConfig: JSON.stringify(initialData.settingsConfig, null, 2),
        onConfigChange,
        initialData,
        initialEnabled: undefined,
      }),
    );

    await waitFor(() => expect(result.current.isLoading).toBe(false));
    await waitFor(() => expect(result.current.useCommonConfig).toBe(true));
  });

  it("Codex：live 刷新换掉已合并的 codexConfig 后仍保持勾选", async () => {
    apiMocks.getCommonConfigSnippet.mockResolvedValue(
      "[tui]\nnotifications = true\n",
    );

    const onConfigChange = vi.fn();
    const storedConfig =
      'model_provider = "custom"\nmodel = "gpt-5"\n\n[tui]\nnotifications = true\n';
    const liveConfig =
      'model_provider = "custom"\nmodel = "gpt-6-live"\n\n[tui]\nnotifications = true\n';
    const storedData = { settingsConfig: { config: storedConfig } };
    const liveData = { settingsConfig: { config: liveConfig } };

    const { result, rerender } = renderHook(
      ({
        codexConfig,
        initialData,
      }: {
        codexConfig: string;
        initialData: { settingsConfig: Record<string, unknown> };
      }) =>
        useCodexCommonConfig({
          codexConfig,
          onConfigChange,
          initialData,
          initialEnabled: true,
        }),
      {
        initialProps: { codexConfig: storedConfig, initialData: storedData },
      },
    );

    await waitFor(() => expect(result.current.isLoading).toBe(false));
    await waitFor(() => expect(result.current.useCommonConfig).toBe(true));
    expect(onConfigChange).not.toHaveBeenCalled();

    await act(async () => {
      rerender({ codexConfig: storedConfig, initialData: liveData });
    });
    await act(async () => {
      rerender({ codexConfig: liveConfig, initialData: liveData });
    });

    expect(result.current.useCommonConfig).toBe(true);
    expect(onConfigChange).not.toHaveBeenCalled();
  });

  it("Codex：用户手动删掉片段后取消勾选", async () => {
    apiMocks.getCommonConfigSnippet.mockResolvedValue(
      "[tui]\nnotifications = true\n",
    );

    const onConfigChange = vi.fn();
    const mergedConfig = 'model = "gpt-5"\n\n[tui]\nnotifications = true\n';
    const initialData = { settingsConfig: { config: mergedConfig } };

    const { result, rerender } = renderHook(
      ({ codexConfig }: { codexConfig: string }) =>
        useCodexCommonConfig({
          codexConfig,
          onConfigChange,
          initialData,
          initialEnabled: true,
        }),
      { initialProps: { codexConfig: mergedConfig } },
    );

    await waitFor(() => expect(result.current.isLoading).toBe(false));
    await waitFor(() => expect(result.current.useCommonConfig).toBe(true));

    await act(async () => {
      rerender({ codexConfig: 'model = "gpt-5"\n' });
    });
    expect(result.current.useCommonConfig).toBe(false);
  });

  it("Gemini：live 刷新换掉已合并的 env 后仍保持勾选", async () => {
    apiMocks.getCommonConfigSnippet.mockResolvedValue(
      JSON.stringify({ GEMINI_MODEL: "gemini-2.5-pro" }),
    );

    const onEnvChange = vi.fn();
    const storedEnv =
      "GEMINI_API_KEY=provider-key\nGEMINI_MODEL=gemini-2.5-pro";
    const liveEnv = "GEMINI_API_KEY=live-key\nGEMINI_MODEL=gemini-2.5-pro";
    const storedData = {
      settingsConfig: {
        env: {
          GEMINI_API_KEY: "provider-key",
          GEMINI_MODEL: "gemini-2.5-pro",
        },
      },
    };
    const liveData = {
      settingsConfig: {
        env: { GEMINI_API_KEY: "live-key", GEMINI_MODEL: "gemini-2.5-pro" },
      },
    };

    const { result, rerender } = renderHook(
      ({
        envValue,
        initialData,
      }: {
        envValue: string;
        initialData: { settingsConfig: Record<string, unknown> };
      }) =>
        useGeminiCommonConfig({
          envValue,
          onEnvChange,
          envStringToObj,
          envObjToString,
          initialData,
          initialEnabled: true,
        }),
      {
        initialProps: { envValue: storedEnv, initialData: storedData },
      },
    );

    await waitFor(() => expect(result.current.isLoading).toBe(false));
    await waitFor(() => expect(result.current.useCommonConfig).toBe(true));
    expect(onEnvChange).not.toHaveBeenCalled();

    await act(async () => {
      rerender({ envValue: storedEnv, initialData: liveData });
    });
    await act(async () => {
      rerender({ envValue: liveEnv, initialData: liveData });
    });

    expect(result.current.useCommonConfig).toBe(true);
    expect(onEnvChange).not.toHaveBeenCalled();
  });

  it("Gemini：用户手动删掉片段后取消勾选", async () => {
    apiMocks.getCommonConfigSnippet.mockResolvedValue(
      JSON.stringify({ GEMINI_MODEL: "gemini-2.5-pro" }),
    );

    const onEnvChange = vi.fn();
    const mergedEnv = "GEMINI_API_KEY=k\nGEMINI_MODEL=gemini-2.5-pro";
    const initialData = {
      settingsConfig: {
        env: { GEMINI_API_KEY: "k", GEMINI_MODEL: "gemini-2.5-pro" },
      },
    };

    const { result, rerender } = renderHook(
      ({ envValue }: { envValue: string }) =>
        useGeminiCommonConfig({
          envValue,
          onEnvChange,
          envStringToObj,
          envObjToString,
          initialData,
          initialEnabled: true,
        }),
      { initialProps: { envValue: mergedEnv } },
    );

    await waitFor(() => expect(result.current.isLoading).toBe(false));
    await waitFor(() => expect(result.current.useCommonConfig).toBe(true));

    await act(async () => {
      rerender({ envValue: "GEMINI_API_KEY=k" });
    });
    expect(result.current.useCommonConfig).toBe(false);
  });
});
