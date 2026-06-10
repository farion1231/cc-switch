import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useCommonConfigSnippet } from "@/components/providers/forms/hooks/useCommonConfigSnippet";
import { useCodexCommonConfig } from "@/components/providers/forms/hooks/useCodexCommonConfig";
import { useGeminiCommonConfig } from "@/components/providers/forms/hooks/useGeminiCommonConfig";

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

describe("common config snippet saving", () => {
  beforeEach(() => {
    getCommonConfigSnippetMock.mockResolvedValue("");
    setCommonConfigSnippetMock.mockResolvedValue(null);
    extractCommonConfigSnippetMock.mockResolvedValue("");
  });

  it("does not persist an invalid Codex common config snippet", async () => {
    const onConfigChange = vi.fn();
    const { result } = renderHook(() =>
      useCodexCommonConfig({
        codexConfig: "model = \"gpt-5\"",
        onConfigChange,
      }),
    );

    await waitFor(() => expect(result.current.isLoading).toBe(false));

    let saved = false;
    act(() => {
      saved = result.current.handleCommonConfigSnippetChange(
        "base_url = https://bad.example/v1",
      );
    });

    expect(saved).toBe(false);
    expect(setCommonConfigSnippetMock).not.toHaveBeenCalled();
    expect(onConfigChange).not.toHaveBeenCalled();
    expect(result.current.commonConfigError).toContain("invalid value");
  });

  it("does not persist an invalid Gemini common config snippet", async () => {
    const onEnvChange = vi.fn();
    const { result } = renderHook(() =>
      useGeminiCommonConfig({
        envValue: "",
        onEnvChange,
        envStringToObj: () => ({}),
        envObjToString: () => "",
      }),
    );

    await waitFor(() => expect(result.current.isLoading).toBe(false));

    let saved = false;
    act(() => {
      saved = result.current.handleCommonConfigSnippetChange(
        JSON.stringify({ GEMINI_MODEL: 123 }),
      );
    });

    expect(saved).toBe(false);
    expect(setCommonConfigSnippetMock).not.toHaveBeenCalled();
    expect(onEnvChange).not.toHaveBeenCalled();
    expect(result.current.commonConfigError).toBe(
      "geminiConfig.commonConfigInvalidValues",
    );
  });

  it("uses the sanitized Claude snippet returned by the backend", async () => {
    const previousSnippet = JSON.stringify(
      {
        includeCoAuthoredBy: false,
      },
      null,
      2,
    );
    const settingsConfig = JSON.stringify(
      {
        env: { CLAUDE_CODE_DISABLE_TERMINAL_TITLE: "1" },
        includeCoAuthoredBy: false,
      },
      null,
      2,
    );
    const rawSnippet = JSON.stringify(
      {
        includeCoAuthoredBy: false,
        hooks: {
          SessionStart: [
            {
              hooks: [
                {
                  type: "command",
                  command: "/missing/bridge",
                },
              ],
            },
          ],
        },
        statusLine: {
          type: "command",
          command: "/missing/statusline",
        },
      },
      null,
      2,
    );
    const sanitizedSnippet = previousSnippet;
    const onConfigChange = vi.fn();

    getCommonConfigSnippetMock.mockResolvedValue(previousSnippet);
    setCommonConfigSnippetMock.mockResolvedValue(sanitizedSnippet);

    const { result } = renderHook(() =>
      useCommonConfigSnippet({
        settingsConfig,
        onConfigChange,
        initialData: {
          settingsConfig: JSON.parse(settingsConfig),
        },
        initialEnabled: true,
      }),
    );

    await waitFor(() => expect(result.current.useCommonConfig).toBe(true));

    await act(async () => {
      await result.current.handleCommonConfigSnippetChange(rawSnippet);
    });

    expect(setCommonConfigSnippetMock).toHaveBeenCalledWith(
      "claude",
      rawSnippet,
    );
    expect(result.current.commonConfigSnippet).toBe(sanitizedSnippet);

    const updatedConfig = JSON.parse(onConfigChange.mock.lastCall?.[0] ?? "{}");
    expect(updatedConfig.includeCoAuthoredBy).toBe(false);
    expect(updatedConfig.hooks).toBeUndefined();
    expect(updatedConfig.statusLine).toBeUndefined();
  });
});
