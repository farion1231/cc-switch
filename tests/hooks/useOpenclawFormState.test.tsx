import { act, renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import {
  OPENCLAW_DEFAULT_USER_AGENT,
  useOpenclawFormState,
} from "@/components/providers/forms/hooks/useOpenclawFormState";

vi.mock("@/lib/query/queries", () => ({
  useProvidersQuery: () => ({ data: { providers: {} } }),
}));

const renderOpenclawFormState = (
  initialSettingsConfig: Record<string, unknown>,
) => {
  let settingsConfig = JSON.stringify(initialSettingsConfig);
  const onSettingsConfigChange = vi.fn((nextConfig: string) => {
    settingsConfig = nextConfig;
  });

  const hook = renderHook(() =>
    useOpenclawFormState({
      appId: "openclaw",
      initialData: { settingsConfig: initialSettingsConfig },
      providerId: "test-provider",
      onSettingsConfigChange,
      getSettingsConfig: () => settingsConfig,
    }),
  );

  return {
    ...hook,
    onSettingsConfigChange,
    getSettingsConfig: () => settingsConfig,
  };
};

describe("useOpenclawFormState User-Agent toggle", () => {
  it("enabling keeps unrelated custom headers", () => {
    const { result, getSettingsConfig } = renderOpenclawFormState({
      baseUrl: "https://api.test",
      headers: { "X-Title": "CC Switch" },
    });

    act(() => result.current.handleOpenclawUserAgentChange(true));

    expect(JSON.parse(getSettingsConfig()).headers).toEqual({
      "X-Title": "CC Switch",
      "User-Agent": OPENCLAW_DEFAULT_USER_AGENT,
    });
  });

  it("disabling removes only User-Agent and keeps unrelated headers", () => {
    const { result, getSettingsConfig } = renderOpenclawFormState({
      baseUrl: "https://api.test",
      headers: {
        "User-Agent": OPENCLAW_DEFAULT_USER_AGENT,
        "X-Title": "CC Switch",
      },
    });

    act(() => result.current.handleOpenclawUserAgentChange(false));

    expect(JSON.parse(getSettingsConfig()).headers).toEqual({
      "X-Title": "CC Switch",
    });
  });

  it("disabling drops the whole headers object when only User-Agent remains", () => {
    const { result, getSettingsConfig } = renderOpenclawFormState({
      baseUrl: "https://api.test",
      headers: { "User-Agent": OPENCLAW_DEFAULT_USER_AGENT },
    });

    act(() => result.current.handleOpenclawUserAgentChange(false));

    expect(JSON.parse(getSettingsConfig()).headers).toBeUndefined();
  });

  it("enabling keeps an existing custom User-Agent value instead of the default", () => {
    const customUserAgent =
      "codex_cli_rs/0.77.0 (Windows 10.0.26100; x86_64) WindowsTerminal";
    const { result, getSettingsConfig } = renderOpenclawFormState({
      baseUrl: "https://api.test",
      headers: { "User-Agent": customUserAgent, "X-Custom": "keep" },
    });

    act(() => result.current.handleOpenclawUserAgentChange(true));

    expect(JSON.parse(getSettingsConfig()).headers).toEqual({
      "User-Agent": customUserAgent,
      "X-Custom": "keep",
    });
  });
});
