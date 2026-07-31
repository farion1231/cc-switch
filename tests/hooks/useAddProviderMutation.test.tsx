import type { ReactNode } from "react";
import { act, renderHook } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useAddProviderMutation } from "@/lib/query/mutations";
import type { Provider } from "@/types";

const apiMocks = vi.hoisted(() => ({
  add: vi.fn(),
  ensureClaudeDesktopOfficialProvider: vi.fn(),
  ensureCodexOfficialProvider: vi.fn(),
  getAll: vi.fn(),
  updateTrayMenu: vi.fn(),
}));

const uuidMocks = vi.hoisted(() => ({
  generateUUID: vi.fn(),
}));

vi.mock("@/lib/api", () => ({
  providersApi: {
    add: (...args: unknown[]) => apiMocks.add(...args),
    ensureClaudeDesktopOfficialProvider: (...args: unknown[]) =>
      apiMocks.ensureClaudeDesktopOfficialProvider(...args),
    ensureCodexOfficialProvider: (...args: unknown[]) =>
      apiMocks.ensureCodexOfficialProvider(...args),
    getAll: (...args: unknown[]) => apiMocks.getAll(...args),
    updateTrayMenu: (...args: unknown[]) => apiMocks.updateTrayMenu(...args),
  },
  sessionsApi: {},
  settingsApi: {},
}));

vi.mock("@/utils/uuid", () => ({
  generateUUID: () => uuidMocks.generateUUID(),
}));

vi.mock("sonner", () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
  },
}));

function createWrapper() {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  });

  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );

  return { wrapper };
}

beforeEach(() => {
  apiMocks.add.mockReset().mockResolvedValue(true);
  apiMocks.ensureClaudeDesktopOfficialProvider
    .mockReset()
    .mockResolvedValue(true);
  apiMocks.ensureCodexOfficialProvider.mockReset().mockResolvedValue(true);
  apiMocks.getAll.mockReset().mockResolvedValue({});
  apiMocks.updateTrayMenu.mockReset().mockResolvedValue(true);
  uuidMocks.generateUUID.mockReset().mockReturnValue("generated-uuid");
});

describe("useAddProviderMutation", () => {
  it("uses the official Qoder provider/model key instead of a generated UUID", async () => {
    const { wrapper } = createWrapper();
    const { result } = renderHook(() => useAddProviderMutation("qodercli"), {
      wrapper,
    });

    const provider = await act(async () =>
      result.current.mutateAsync({
        name: "DeepSeek",
        providerKey: "deepseek",
        category: "cn_official",
        settingsConfig: {
          provider: "deepseek",
          apiKey: "sk-test",
          models: [
            {
              model: "deepseek-v4-pro-pg",
              type: "pg",
              format: "openai",
            },
          ],
        },
      }),
    );

    expect(provider.id).toBe("deepseek/deepseek-v4-pro-pg");
    expect(uuidMocks.generateUUID).not.toHaveBeenCalled();
    expect(apiMocks.add).toHaveBeenCalledWith(
      expect.objectContaining({
        id: "deepseek/deepseek-v4-pro-pg",
        name: "DeepSeek",
      }),
      "qodercli",
      undefined,
    );
  });

  it("keeps two models from the same Qoder supplier as separate records", async () => {
    const { wrapper } = createWrapper();
    const { result } = renderHook(() => useAddProviderMutation("qodercli"), {
      wrapper,
    });

    const makeInput = (model: string) => ({
      name: "DeepSeek",
      providerKey: "deepseek",
      category: "cn_official" as const,
      settingsConfig: {
        provider: "deepseek",
        apiKey: "sk-test",
        models: [{ model, type: "pg", format: "openai" }],
      },
    });

    const pro = await act(async () =>
      result.current.mutateAsync(makeInput("deepseek-v4-pro-pg")),
    );
    const flash = await act(async () =>
      result.current.mutateAsync(makeInput("deepseek-v4-flash-pg")),
    );

    expect(pro.id).toBe("deepseek/deepseek-v4-pro-pg");
    expect(flash.id).toBe("deepseek/deepseek-v4-flash-pg");
    expect(apiMocks.add).toHaveBeenCalledTimes(2);
    expect(
      apiMocks.add.mock.calls.map(([saved]) => (saved as Provider).id),
    ).toEqual(["deepseek/deepseek-v4-pro-pg", "deepseek/deepseek-v4-flash-pg"]);
  });

  it("uses a manually entered model ID for a supported Qoder provider", async () => {
    const { wrapper } = createWrapper();
    const { result } = renderHook(() => useAddProviderMutation("qodercli"), {
      wrapper,
    });

    const provider = await act(async () =>
      result.current.mutateAsync({
        name: "Kimi custom model",
        providerKey: "kimi",
        category: "cn_official",
        settingsConfig: {
          provider: "kimi",
          apiKey: "sk-test",
          models: [
            {
              model: "moonshot-v1-custom",
              type: "cp",
              format: "openai",
            },
          ],
        },
      }),
    );

    expect(provider.id).toBe("kimi/moonshot-v1-custom");
    expect(apiMocks.add).toHaveBeenCalledWith(
      expect.objectContaining({
        id: "kimi/moonshot-v1-custom",
        name: "Kimi custom model",
      }),
      "qodercli",
      undefined,
    );
  });

  it("duplicates Claude Desktop official providers with a fresh id", async () => {
    const { wrapper } = createWrapper();
    const { result } = renderHook(
      () => useAddProviderMutation("claude-desktop"),
      { wrapper },
    );

    const duplicatedProvider = await act(async () =>
      result.current.mutateAsync({
        name: "Claude Desktop Official copy",
        settingsConfig: { env: {} },
        category: "official",
      }),
    );

    expect(apiMocks.ensureClaudeDesktopOfficialProvider).not.toHaveBeenCalled();
    expect(apiMocks.add).toHaveBeenCalledTimes(1);
    expect(apiMocks.add).toHaveBeenCalledWith(
      expect.objectContaining({
        id: "generated-uuid",
        name: "Claude Desktop Official copy",
        category: "official",
      }),
      "claude-desktop",
      undefined,
    );
    expect(duplicatedProvider.id).toBe("generated-uuid");
    expect(duplicatedProvider.id).not.toBe("claude-desktop-official");
  });

  it("returns the persisted seed row for the Claude Desktop official preset", async () => {
    const seedProvider: Provider = {
      id: "claude-desktop-official",
      name: "Claude Desktop Official",
      settingsConfig: { env: {} },
      websiteUrl: "https://claude.ai/download",
      category: "official",
      icon: "anthropic",
      iconColor: "#D4915D",
      createdAt: 123,
    };
    apiMocks.getAll.mockResolvedValueOnce({
      "claude-desktop-official": seedProvider,
    });
    const { wrapper } = createWrapper();
    const { result } = renderHook(
      () => useAddProviderMutation("claude-desktop"),
      { wrapper },
    );

    const persistedProvider = await act(async () =>
      result.current.mutateAsync({
        name: "Renamed by form",
        settingsConfig: { env: { ignored: true } },
        websiteUrl: "https://example.invalid",
        category: "official",
        icon: "custom-icon",
        ensureClaudeDesktopOfficialSeed: true,
      }),
    );

    expect(apiMocks.ensureClaudeDesktopOfficialProvider).toHaveBeenCalledTimes(
      1,
    );
    expect(apiMocks.getAll).toHaveBeenCalledWith("claude-desktop");
    expect(apiMocks.add).not.toHaveBeenCalled();
    expect(persistedProvider).toEqual(seedProvider);
  });

  it("recreates and returns the fixed Codex official seed", async () => {
    const seedProvider: Provider = {
      id: "codex-official",
      name: "OpenAI Official",
      settingsConfig: { auth: {}, config: "" },
      category: "official",
    };
    apiMocks.getAll.mockResolvedValueOnce({
      "codex-official": seedProvider,
    });
    const { wrapper } = createWrapper();
    const { result } = renderHook(() => useAddProviderMutation("codex"), {
      wrapper,
    });

    const persistedProvider = await act(async () =>
      result.current.mutateAsync({
        name: "OpenAI Official",
        settingsConfig: { auth: {}, config: "" },
        category: "official",
        ensureCodexOfficialSeed: true,
      }),
    );

    expect(apiMocks.ensureCodexOfficialProvider).toHaveBeenCalledTimes(1);
    expect(apiMocks.getAll).toHaveBeenCalledWith("codex");
    expect(apiMocks.add).not.toHaveBeenCalled();
    expect(persistedProvider).toEqual(seedProvider);
  });
});
