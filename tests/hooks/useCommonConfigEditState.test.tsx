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

describe("编辑供应商时保留 meta.commonConfigEnabled 勾选状态", () => {
  beforeEach(() => {
    apiMocks.getCommonConfigSnippet.mockResolvedValue("");
    apiMocks.setCommonConfigSnippet.mockResolvedValue(undefined);
    apiMocks.extractCommonConfigSnippet.mockResolvedValue("");
    apiMocks.updateTomlCommonConfigSnippet.mockImplementation(
      async (configToml: string) => configToml,
    );
  });

  it("Claude：快照不含片段时仍保持勾选，后续外部刷新快照也不会取消勾选", async () => {
    apiMocks.getCommonConfigSnippet.mockResolvedValue(
      JSON.stringify({ plugin: "shared" }),
    );

    const onConfigChange = vi.fn();
    const initialData = { settingsConfig: { apiKey: "provider-key" } };
    const storedConfig = JSON.stringify(initialData.settingsConfig, null, 2);

    const { result, rerender } = renderHook(
      ({ settingsConfig }: { settingsConfig: string }) =>
        useCommonConfigSnippet({
          settingsConfig,
          onConfigChange,
          initialData,
          initialEnabled: true,
        }),
      { initialProps: { settingsConfig: storedConfig } },
    );

    await waitFor(() => expect(result.current.isLoading).toBe(false));
    await waitFor(() => expect(result.current.useCommonConfig).toBe(true));

    // 模拟表单先显示合并后的预览，随后 EditProviderDialog 读取 live 失败，
    // 重新以数据库快照（不含通用配置）初始化表单
    await act(async () => {
      rerender({
        settingsConfig: JSON.stringify(
          { apiKey: "provider-key", plugin: "shared" },
          null,
          2,
        ),
      });
    });
    expect(result.current.useCommonConfig).toBe(true);

    await act(async () => {
      rerender({ settingsConfig: storedConfig });
    });
    expect(result.current.useCommonConfig).toBe(true);

    // 用户仍可正常取消勾选
    await act(async () => {
      result.current.handleCommonConfigToggle(false);
    });
    expect(result.current.useCommonConfig).toBe(false);
  });

  it("Codex：异步合并片段期间外部改写 config.toml 不会把勾选翻成 false", async () => {
    apiMocks.getCommonConfigSnippet.mockResolvedValue(
      "[tui]\nnotifications = true\n",
    );

    let resolveMerge: ((value: string) => void) | undefined;
    apiMocks.updateTomlCommonConfigSnippet.mockImplementationOnce(
      () =>
        new Promise<string>((resolve) => {
          resolveMerge = resolve;
        }),
    );

    const onConfigChange = vi.fn();
    const initialData = {
      settingsConfig: { config: 'model = "gpt-5"\n' },
    };

    const { result, rerender } = renderHook(
      ({ codexConfig }: { codexConfig: string }) =>
        useCodexCommonConfig({
          codexConfig,
          onConfigChange,
          initialData,
          initialEnabled: true,
        }),
      { initialProps: { codexConfig: 'model = "gpt-5"\n' } },
    );

    await waitFor(() => expect(result.current.isLoading).toBe(false));
    await waitFor(() => expect(result.current.useCommonConfig).toBe(true));

    // 合并请求在飞期间，外部把 config.toml 替换成不含通用配置的内容
    await act(async () => {
      rerender({ codexConfig: 'model = "gpt-6-user-edit"\n' });
      resolveMerge?.('model = "gpt-5"\n\n[tui]\nnotifications = true\n');
    });

    await act(async () => {
      await Promise.resolve();
    });
    expect(result.current.useCommonConfig).toBe(true);
  });

  it("Gemini：快照 env 不含片段时仍保持勾选，后续外部刷新 env 也不会取消勾选", async () => {
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
    const initialData = {
      settingsConfig: { env: { GEMINI_API_KEY: "provider-key" } },
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
      { initialProps: { envValue: "GEMINI_API_KEY=provider-key" } },
    );

    await waitFor(() => expect(result.current.isLoading).toBe(false));
    await waitFor(() => expect(result.current.useCommonConfig).toBe(true));

    await act(async () => {
      rerender({
        envValue: "GEMINI_API_KEY=provider-key\nGEMINI_MODEL=gemini-2.5-pro",
      });
    });
    expect(result.current.useCommonConfig).toBe(true);

    await act(async () => {
      rerender({ envValue: "GEMINI_API_KEY=provider-key" });
    });
    expect(result.current.useCommonConfig).toBe(true);

    await act(async () => {
      result.current.handleCommonConfigToggle(false);
    });
    expect(result.current.useCommonConfig).toBe(false);
  });
});
