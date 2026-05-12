import React from "react";
import { renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { parse as parseToml } from "smol-toml";
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

const envStringToObj = (envString: string): Record<string, string> =>
  envString
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean)
    .reduce<Record<string, string>>((acc, line) => {
      const [key, ...rest] = line.split("=");
      if (key) {
        acc[key] = rest.join("=");
      }
      return acc;
    }, {});

const envObjToString = (envObj: Record<string, unknown>): string =>
  Object.entries(envObj)
    .map(([key, value]) => `${key}=${String(value)}`)
    .join("\n");

describe("common config provider precedence", () => {
  beforeEach(() => {
    getCommonConfigSnippetMock.mockResolvedValue("");
    setCommonConfigSnippetMock.mockResolvedValue(undefined);
    extractCommonConfigSnippetMock.mockResolvedValue("");
  });

  it("keeps Claude provider overrides visible in edit mode", async () => {
    const snippet = JSON.stringify(
      {
        includeCoAuthoredBy: false,
        nested: {
          shared: "common",
          commonOnly: "enabled",
        },
      },
      null,
      2,
    );
    const initialSettings = {
      includeCoAuthoredBy: true,
      nested: {
        shared: "provider",
        providerOnly: "keep",
      },
    };
    getCommonConfigSnippetMock.mockResolvedValue(snippet);

    const { result } = renderHook(() => {
      const [config, setConfig] = React.useState(
        JSON.stringify(initialSettings, null, 2),
      );
      const hook = useCommonConfigSnippet({
        settingsConfig: config,
        onConfigChange: setConfig,
        initialData: { settingsConfig: initialSettings },
        initialEnabled: true,
      });

      return { ...hook, config };
    });

    await waitFor(() => expect(result.current.isLoading).toBe(false));
    await waitFor(() =>
      expect(JSON.parse(result.current.config)).toEqual({
        includeCoAuthoredBy: true,
        nested: {
          shared: "provider",
          commonOnly: "enabled",
          providerOnly: "keep",
        },
      }),
    );
    await waitFor(() => expect(result.current.useCommonConfig).toBe(true));
  });

  it("keeps Codex provider overrides visible in edit mode", async () => {
    const snippet = [
      'model = "common-model"',
      "",
      "[profiles.default]",
      'sandbox_mode = "workspace-write"',
      "",
    ].join("\n");
    const initialConfig = [
      'model = "provider-model"',
      "",
      "[profiles.default]",
      'approval_policy = "never"',
      "",
    ].join("\n");
    getCommonConfigSnippetMock.mockResolvedValue(snippet);

    const { result } = renderHook(() => {
      const [config, setConfig] = React.useState(initialConfig);
      const hook = useCodexCommonConfig({
        codexConfig: config,
        onConfigChange: setConfig,
        initialData: {
          settingsConfig: {
            config: initialConfig,
          },
        },
        initialEnabled: true,
      });

      return { ...hook, config };
    });

    await waitFor(() => expect(result.current.isLoading).toBe(false));
    await waitFor(() =>
      expect(parseToml(result.current.config)).toEqual({
        model: "provider-model",
        profiles: {
          default: {
            approval_policy: "never",
            sandbox_mode: "workspace-write",
          },
        },
      }),
    );
    await waitFor(() => expect(result.current.useCommonConfig).toBe(true));
  });

  it("keeps Gemini provider overrides visible in edit mode", async () => {
    const snippet = JSON.stringify({
      GEMINI_MODEL: "common-model",
      HTTPS_PROXY: "http://proxy.example",
    });
    const initialEnv = {
      GEMINI_MODEL: "provider-model",
      PROVIDER_ONLY: "keep",
    };
    getCommonConfigSnippetMock.mockResolvedValue(snippet);

    const { result } = renderHook(() => {
      const [env, setEnv] = React.useState(
        envObjToString(initialEnv as Record<string, unknown>),
      );
      const hook = useGeminiCommonConfig({
        envValue: env,
        onEnvChange: setEnv,
        envStringToObj,
        envObjToString,
        initialData: {
          settingsConfig: {
            env: initialEnv,
          },
        },
        initialEnabled: true,
      });

      return { ...hook, env };
    });

    await waitFor(() => expect(result.current.isLoading).toBe(false));
    await waitFor(() =>
      expect(envStringToObj(result.current.env)).toEqual({
        GEMINI_MODEL: "provider-model",
        HTTPS_PROXY: "http://proxy.example",
        PROVIDER_ONLY: "keep",
      }),
    );
    await waitFor(() => expect(result.current.useCommonConfig).toBe(true));
  });
});
