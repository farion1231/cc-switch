import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useCommonConfigSnippet } from "@/components/providers/forms/hooks/useCommonConfigSnippet";
import { useCodexCommonConfig } from "@/components/providers/forms/hooks/useCodexCommonConfig";
import { useGeminiCommonConfig } from "@/components/providers/forms/hooks/useGeminiCommonConfig";

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

describe("编辑供应商时保持勾选状态与配置预览一致", () => {
  beforeEach(() => {
    apiMocks.getCommonConfigSnippet.mockResolvedValue("");
    apiMocks.setCommonConfigSnippet.mockResolvedValue(undefined);
    apiMocks.extractCommonConfigSnippet.mockResolvedValue("");
    apiMocks.updateTomlCommonConfigSnippet.mockImplementation(
      async (configToml: string) => configToml,
    );
  });

  it("Claude：live 刷新重置为不含片段的快照后，仍保持勾选并把片段合并回预览", async () => {
    apiMocks.getCommonConfigSnippet.mockResolvedValue(
      JSON.stringify({ plugin: "shared" }),
    );

    const onConfigChange = vi.fn();
    const storedData = { settingsConfig: { apiKey: "provider-key" } };
    const storedConfig = JSON.stringify(storedData.settingsConfig, null, 2);
    const liveData = { settingsConfig: { apiKey: "live-key" } };
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
    expect(JSON.parse(onConfigChange.mock.calls.at(-1)?.[0])).toEqual({
      apiKey: "provider-key",
      plugin: "shared",
    });

    // 模拟 EditProviderDialog 读到 live 配置后替换 initialData 并重置表单
    await act(async () => {
      rerender({ settingsConfig: liveConfig, initialData: liveData });
    });

    expect(result.current.useCommonConfig).toBe(true);
    expect(JSON.parse(onConfigChange.mock.calls.at(-1)?.[0])).toEqual({
      apiKey: "live-key",
      plugin: "shared",
    });

    // 模拟父组件应用了上面的合并预览
    await act(async () => {
      rerender({
        settingsConfig: JSON.stringify(
          { apiKey: "live-key", plugin: "shared" },
          null,
          2,
        ),
        initialData: liveData,
      });
    });
    expect(result.current.useCommonConfig).toBe(true);

    // 用户手动从配置中移除片段时，仍按内容同步为未勾选
    await act(async () => {
      rerender({
        settingsConfig: JSON.stringify({ apiKey: "live-key" }, null, 2),
        initialData: liveData,
      });
    });
    expect(result.current.useCommonConfig).toBe(false);
  });

  it("Codex：live 刷新重置 codexConfig 后，仍保持勾选并把片段合并回预览", async () => {
    apiMocks.getCommonConfigSnippet.mockResolvedValue(
      "[tui]\nnotifications = true\n",
    );
    apiMocks.updateTomlCommonConfigSnippet.mockImplementation(
      async (configToml: string, _snippet: string, enabled: boolean) =>
        enabled ? `${configToml}\n\n[tui]\nnotifications = true\n` : configToml,
    );

    const onConfigChange = vi.fn();
    const storedData = {
      settingsConfig: {
        config: 'model_provider = "custom"\nmodel = "gpt-5"\n',
      },
    };
    const liveData = {
      settingsConfig: {
        config: 'model_provider = "custom"\nmodel = "gpt-6-live"\n',
      },
    };

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
        initialProps: {
          codexConfig: storedData.settingsConfig.config as string,
          initialData: storedData,
        },
      },
    );

    await waitFor(() => expect(result.current.isLoading).toBe(false));
    await waitFor(() => expect(result.current.useCommonConfig).toBe(true));
    expect(onConfigChange.mock.calls.at(-1)?.[0]).toContain("[tui]");
    expect(onConfigChange.mock.calls.at(-1)?.[0]).toContain('model = "gpt-5"');

    await act(async () => {
      rerender({
        codexConfig: liveData.settingsConfig.config as string,
        initialData: liveData,
      });
    });

    await waitFor(() =>
      expect(onConfigChange.mock.calls.at(-1)?.[0]).toContain(
        'model = "gpt-6-live"',
      ),
    );
    expect(onConfigChange.mock.calls.at(-1)?.[0]).toContain("[tui]");
    expect(result.current.useCommonConfig).toBe(true);
  });

  it("Codex：codexConfig 晚于片段加载初始化时，等待稳定后再合并片段", async () => {
    apiMocks.getCommonConfigSnippet.mockResolvedValue(
      "[tui]\nnotifications = true\n",
    );
    apiMocks.updateTomlCommonConfigSnippet.mockImplementation(
      async (configToml: string, _snippet: string, enabled: boolean) =>
        enabled ? `${configToml}\n\n[tui]\nnotifications = true\n` : configToml,
    );

    const onConfigChange = vi.fn();
    const initialData = {
      settingsConfig: {
        config: 'model_provider = "custom"\nmodel = "gpt-5"\n',
      },
    };

    const { result, rerender } = renderHook(
      ({ codexConfig }: { codexConfig: string }) =>
        useCodexCommonConfig({
          codexConfig,
          onConfigChange,
          initialData,
          initialEnabled: true,
        }),
      { initialProps: { codexConfig: "" } },
    );

    await waitFor(() => expect(result.current.isLoading).toBe(false));
    await waitFor(() => expect(result.current.useCommonConfig).toBe(true));

    // 模拟 useCodexConfigState 在片段加载完成后才把 config.toml 填进来
    await act(async () => {
      rerender({ codexConfig: initialData.settingsConfig.config as string });
    });

    await waitFor(() =>
      expect(onConfigChange.mock.calls.at(-1)?.[0]).toContain("[tui]"),
    );
    expect(onConfigChange.mock.calls.at(-1)?.[0]).toContain('model = "gpt-5"');
    expect(result.current.useCommonConfig).toBe(true);
  });

  it("Gemini：live 刷新重置 env 后，仍保持勾选并把片段合并回预览", async () => {
    apiMocks.getCommonConfigSnippet.mockResolvedValue(
      JSON.stringify({ GEMINI_MODEL: "gemini-2.5-pro" }),
    );

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

    const onEnvChange = vi.fn();
    const storedData = {
      settingsConfig: { env: { GEMINI_API_KEY: "provider-key" } },
    };
    const liveData = {
      settingsConfig: { env: { GEMINI_API_KEY: "live-key" } },
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
        initialProps: {
          envValue: "GEMINI_API_KEY=provider-key",
          initialData: storedData,
        },
      },
    );

    await waitFor(() => expect(result.current.isLoading).toBe(false));
    await waitFor(() => expect(result.current.useCommonConfig).toBe(true));
    expect(onEnvChange.mock.calls.at(-1)?.[0]).toContain(
      "GEMINI_MODEL=gemini-2.5-pro",
    );

    await act(async () => {
      rerender({ envValue: "GEMINI_API_KEY=live-key", initialData: liveData });
    });

    expect(result.current.useCommonConfig).toBe(true);
    expect(onEnvChange.mock.calls.at(-1)?.[0]).toContain(
      "GEMINI_API_KEY=live-key",
    );
    expect(onEnvChange.mock.calls.at(-1)?.[0]).toContain(
      "GEMINI_MODEL=gemini-2.5-pro",
    );
  });
});
