import { renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useInitialDataCommonConfig } from "@/components/providers/forms/hooks/useInitialDataCommonConfig";

const apiMocks = vi.hoisted(() => ({
  getCommonConfigSnippet: vi.fn(),
  updateTomlCommonConfigSnippet: vi.fn(),
}));

vi.mock("@/lib/api", () => ({
  configApi: {
    getCommonConfigSnippet: apiMocks.getCommonConfigSnippet,
    updateTomlCommonConfigSnippet: apiMocks.updateTomlCommonConfigSnippet,
  },
}));

describe("useInitialDataCommonConfig（数据层合并）", () => {
  beforeEach(() => {
    apiMocks.getCommonConfigSnippet.mockReset();
    apiMocks.updateTomlCommonConfigSnippet.mockReset();
    apiMocks.getCommonConfigSnippet.mockResolvedValue("");
    apiMocks.updateTomlCommonConfigSnippet.mockImplementation(
      async (configToml: string) => configToml,
    );
    window.localStorage.clear();
  });

  it("Claude：flag 为 true 时把片段合并进快照", async () => {
    apiMocks.getCommonConfigSnippet.mockResolvedValue(
      JSON.stringify({ plugin: "shared" }),
    );
    const settingsConfig = { apiKey: "provider-key" };

    const { result } = renderHook(() =>
      useInitialDataCommonConfig({
        appId: "claude",
        commonConfigEnabled: true,
        settingsConfig,
      }),
    );

    await waitFor(() =>
      expect(result.current).toEqual({
        apiKey: "provider-key",
        plugin: "shared",
      }),
    );
  });

  it("flag 不为 true 时不合并，也不去读片段", async () => {
    apiMocks.getCommonConfigSnippet.mockResolvedValue(
      JSON.stringify({ plugin: "shared" }),
    );
    const settingsConfig = { apiKey: "provider-key" };

    const { result } = renderHook(() =>
      useInitialDataCommonConfig({
        appId: "claude",
        commonConfigEnabled: false,
        settingsConfig,
      }),
    );

    await waitFor(() => expect(result.current).toBe(settingsConfig));
    expect(apiMocks.getCommonConfigSnippet).not.toHaveBeenCalled();
  });

  it("片段为空时保持同一引用", async () => {
    const settingsConfig = { apiKey: "provider-key" };

    const { result } = renderHook(() =>
      useInitialDataCommonConfig({
        appId: "claude",
        commonConfigEnabled: true,
        settingsConfig,
      }),
    );

    await waitFor(() =>
      expect(apiMocks.getCommonConfigSnippet).toHaveBeenCalledWith("claude"),
    );
    expect(result.current).toBe(settingsConfig);
  });

  it("Codex：走后端 toml_edit 合并 config 字段", async () => {
    apiMocks.getCommonConfigSnippet.mockResolvedValue(
      "[tui]\nnotifications = true\n",
    );
    apiMocks.updateTomlCommonConfigSnippet.mockImplementation(
      async (configToml: string, _snippet: string, enabled: boolean) =>
        enabled ? `${configToml}\n[tui]\nnotifications = true\n` : configToml,
    );
    const settingsConfig = { config: 'model = "gpt-5"\n', auth: {} };

    const { result } = renderHook(() =>
      useInitialDataCommonConfig({
        appId: "codex",
        commonConfigEnabled: true,
        settingsConfig,
      }),
    );

    await waitFor(() => expect(result.current.config).toContain("[tui]"));
    expect(result.current.config).toContain('model = "gpt-5"');
    // 非 config 字段原样保留
    expect(result.current.auth).toEqual({});
  });

  it("Codex：后端合并失败时退回原快照", async () => {
    apiMocks.getCommonConfigSnippet.mockResolvedValue("[tui]\n");
    apiMocks.updateTomlCommonConfigSnippet.mockRejectedValue(
      new Error("toml parse failed"),
    );
    const settingsConfig = { config: 'model = "gpt-5"\n' };

    const { result } = renderHook(() =>
      useInitialDataCommonConfig({
        appId: "codex",
        commonConfigEnabled: true,
        settingsConfig,
      }),
    );

    await waitFor(() =>
      expect(apiMocks.updateTomlCommonConfigSnippet).toHaveBeenCalled(),
    );
    expect(result.current).toBe(settingsConfig);
  });

  it("Gemini：合并进 env", async () => {
    apiMocks.getCommonConfigSnippet.mockResolvedValue(
      JSON.stringify({ GEMINI_MODEL: "gemini-2.5-pro" }),
    );
    const settingsConfig = { env: { GEMINI_API_KEY: "k" } };

    const { result } = renderHook(() =>
      useInitialDataCommonConfig({
        appId: "gemini",
        commonConfigEnabled: true,
        settingsConfig,
      }),
    );

    await waitFor(() =>
      expect(result.current.env).toEqual({
        GEMINI_API_KEY: "k",
        GEMINI_MODEL: "gemini-2.5-pro",
      }),
    );
  });

  it("Gemini：片段带凭据键时整体不合并", async () => {
    apiMocks.getCommonConfigSnippet.mockResolvedValue(
      JSON.stringify({ GEMINI_MODEL: "gemini-2.5-pro", GOOGLE_API_KEY: "s" }),
    );
    const settingsConfig = { env: { GEMINI_API_KEY: "k" } };

    const { result } = renderHook(() =>
      useInitialDataCommonConfig({
        appId: "gemini",
        commonConfigEnabled: true,
        settingsConfig,
      }),
    );

    await waitFor(() =>
      expect(apiMocks.getCommonConfigSnippet).toHaveBeenCalledWith("gemini"),
    );
    // 凭据绝不能被合并进另一个供应商的 env 预览
    expect(result.current).toBe(settingsConfig);
  });

  it("Gemini：片段值不是字符串时整体不合并", async () => {
    apiMocks.getCommonConfigSnippet.mockResolvedValue(
      JSON.stringify({ GEMINI_MODEL: 42 }),
    );
    const settingsConfig = { env: {} };

    const { result } = renderHook(() =>
      useInitialDataCommonConfig({
        appId: "gemini",
        commonConfigEnabled: true,
        settingsConfig,
      }),
    );

    await waitFor(() =>
      expect(apiMocks.getCommonConfigSnippet).toHaveBeenCalledWith("gemini"),
    );
    expect(result.current).toBe(settingsConfig);
  });

  it("Claude：片段不是合法 JSON 时不合并", async () => {
    apiMocks.getCommonConfigSnippet.mockResolvedValue("{ not json");
    const settingsConfig = { apiKey: "provider-key" };

    const { result } = renderHook(() =>
      useInitialDataCommonConfig({
        appId: "claude",
        commonConfigEnabled: true,
        settingsConfig,
      }),
    );

    await waitFor(() =>
      expect(apiMocks.getCommonConfigSnippet).toHaveBeenCalledWith("claude"),
    );
    expect(result.current).toBe(settingsConfig);
  });

  it("config.json 为空时回退到 localStorage 遗留片段", async () => {
    window.localStorage.setItem(
      "cc-switch:common-config-snippet",
      JSON.stringify({ plugin: "legacy" }),
    );
    const settingsConfig = { apiKey: "provider-key" };

    const { result } = renderHook(() =>
      useInitialDataCommonConfig({
        appId: "claude",
        commonConfigEnabled: true,
        settingsConfig,
      }),
    );

    await waitFor(() =>
      expect(result.current).toEqual({
        apiKey: "provider-key",
        plugin: "legacy",
      }),
    );
    // 迁移与清理仍由表单内的 hook 负责，这里只读不写
    expect(
      window.localStorage.getItem("cc-switch:common-config-snippet"),
    ).not.toBeNull();
  });

  it("enabled 为 false 时不合并", async () => {
    apiMocks.getCommonConfigSnippet.mockResolvedValue(
      JSON.stringify({ plugin: "shared" }),
    );
    const settingsConfig = { apiKey: "provider-key" };

    const { result } = renderHook(() =>
      useInitialDataCommonConfig({
        appId: "claude",
        commonConfigEnabled: true,
        settingsConfig,
        enabled: false,
      }),
    );

    await waitFor(() => expect(result.current).toBe(settingsConfig));
    expect(apiMocks.getCommonConfigSnippet).not.toHaveBeenCalled();
  });

  it("快照变化时不会把上一份合并结果当成当前值", async () => {
    apiMocks.getCommonConfigSnippet.mockResolvedValue(
      JSON.stringify({ plugin: "shared" }),
    );
    const stored = { apiKey: "stored" };
    const live = { apiKey: "live" };

    const { result, rerender } = renderHook(
      ({ settingsConfig }: { settingsConfig: Record<string, unknown> }) =>
        useInitialDataCommonConfig({
          appId: "claude",
          commonConfigEnabled: true,
          settingsConfig,
        }),
      { initialProps: { settingsConfig: stored } },
    );

    await waitFor(() =>
      expect(result.current).toEqual({
        apiKey: "stored",
        plugin: "shared",
      }),
    );

    rerender({ settingsConfig: live });
    // 合并尚未完成时交出入参本身，绝不能显示上一个供应商快照的内容
    expect(result.current.apiKey).toBe("live");

    await waitFor(() =>
      expect(result.current).toEqual({
        apiKey: "live",
        plugin: "shared",
      }),
    );
  });
});
