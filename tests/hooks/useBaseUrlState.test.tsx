import { renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { useBaseUrlState } from "@/components/providers/forms/hooks/useBaseUrlState";

describe("useBaseUrlState", () => {
  it.each([
    { base_url: "https://base-url.example/v1" },
    { baseURL: "https://base-url-camel.example/v1" },
    { apiEndpoint: "https://api-endpoint.example/v1" },
    { apiEndpoint: { url: "https://api-endpoint-object.example/v1" } },
  ])("hydrates Claude base URL from fallback config: %j", (settingsConfig) => {
    const { result } = renderHook(() =>
      useBaseUrlState({
        appType: "claude",
        category: "third_party",
        settingsConfig: JSON.stringify(settingsConfig),
        codexConfig: "",
        onSettingsConfigChange: vi.fn(),
        onCodexConfigChange: vi.fn(),
      }),
    );

    const expected =
      typeof settingsConfig.apiEndpoint === "string"
        ? settingsConfig.apiEndpoint
        : (settingsConfig.apiEndpoint?.url ??
          settingsConfig.base_url ??
          settingsConfig.baseURL);
    expect(result.current.baseUrl).toBe(expected);
  });
});
