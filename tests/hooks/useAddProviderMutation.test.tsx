import type { ReactNode } from "react";
import { act, renderHook } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useAddProviderMutation } from "@/lib/query/mutations";
import { GROKBUILD_OFFICIAL_PROVIDER_ID } from "@/utils/providerCapabilities";
import type { Provider } from "@/types";

const apiMocks = vi.hoisted(() => ({
  add: vi.fn(),
  ensureClaudeDesktopOfficialProvider: vi.fn(),
  ensureGrokBuildOfficialProvider: vi.fn(),
  getAll: vi.fn(),
  updateTrayMenu: vi.fn(),
}));

const uuidMocks = vi.hoisted(() => ({
  generateUUID: vi.fn(),
}));

const toastMocks = vi.hoisted(() => ({
  success: vi.fn(),
  error: vi.fn(),
  warning: vi.fn(),
  info: vi.fn(),
}));

vi.mock("@/lib/api", () => ({
  providersApi: {
    add: (...args: unknown[]) => apiMocks.add(...args),
    ensureClaudeDesktopOfficialProvider: (...args: unknown[]) =>
      apiMocks.ensureClaudeDesktopOfficialProvider(...args),
    ensureGrokBuildOfficialProvider: (...args: unknown[]) =>
      apiMocks.ensureGrokBuildOfficialProvider(...args),
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
  toast: toastMocks,
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
  apiMocks.ensureGrokBuildOfficialProvider.mockReset().mockResolvedValue(true);
  apiMocks.getAll.mockReset().mockResolvedValue({});
  apiMocks.updateTrayMenu.mockReset().mockResolvedValue(true);
  uuidMocks.generateUUID.mockReset().mockReturnValue("generated-uuid");
  toastMocks.success.mockReset();
  toastMocks.error.mockReset();
  toastMocks.warning.mockReset();
  toastMocks.info.mockReset();
});

describe("useAddProviderMutation", () => {
  it("duplicates Claude Desktop official providers with a fresh id", async () => {
    const { wrapper } = createWrapper();
    const { result } = renderHook(
      () => useAddProviderMutation("claude-desktop"),
      { wrapper },
    );

    const added = await act(async () =>
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
    expect(added.provider.id).toBe("generated-uuid");
    expect(added.provider.id).not.toBe("claude-desktop-official");
    expect(added.alreadyExisted).toBe(false);
    expect(toastMocks.success).toHaveBeenCalledTimes(1);
    expect(toastMocks.info).not.toHaveBeenCalled();
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

    const added = await act(async () =>
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
    expect(added.provider).toEqual(seedProvider);
    expect(added.alreadyExisted).toBe(false);
    expect(toastMocks.success).toHaveBeenCalledTimes(1);
    expect(toastMocks.info).not.toHaveBeenCalled();
  });

  it("reports the existing Claude Desktop official provider instead of a false 'added'", async () => {
    // The official provider is seeded once per database, so ensure* returns
    // false (no new row inserted). Adding it again must NOT claim success.
    apiMocks.ensureClaudeDesktopOfficialProvider.mockResolvedValueOnce(false);
    const seedProvider: Provider = {
      id: "claude-desktop-official",
      name: "Claude Desktop Official",
      settingsConfig: { env: {} },
      category: "official",
    };
    apiMocks.getAll.mockResolvedValueOnce({
      "claude-desktop-official": seedProvider,
    });
    const { wrapper } = createWrapper();
    const { result } = renderHook(
      () => useAddProviderMutation("claude-desktop"),
      { wrapper },
    );

    const added = await act(async () =>
      result.current.mutateAsync({
        name: "Claude Desktop Official",
        settingsConfig: { env: {} },
        category: "official",
        ensureClaudeDesktopOfficialSeed: true,
      }),
    );

    expect(apiMocks.add).not.toHaveBeenCalled();
    expect(added.provider).toEqual(seedProvider);
    expect(added.alreadyExisted).toBe(true);
    expect(toastMocks.success).not.toHaveBeenCalled();
    expect(toastMocks.info).toHaveBeenCalledTimes(1);
  });

  it("reports the existing Grok Build official provider instead of a false 'added'", async () => {
    apiMocks.ensureGrokBuildOfficialProvider.mockResolvedValueOnce(false);
    const seedProvider: Provider = {
      id: GROKBUILD_OFFICIAL_PROVIDER_ID,
      name: "Grok Build Official",
      settingsConfig: {},
      category: "official",
    };
    apiMocks.getAll.mockResolvedValueOnce({
      [GROKBUILD_OFFICIAL_PROVIDER_ID]: seedProvider,
    });
    const { wrapper } = createWrapper();
    const { result } = renderHook(() => useAddProviderMutation("grokbuild"), {
      wrapper,
    });

    const added = await act(async () =>
      result.current.mutateAsync({
        name: "Grok Build Official",
        settingsConfig: {},
        category: "official",
        ensureGrokBuildOfficialSeed: true,
      }),
    );

    expect(apiMocks.ensureGrokBuildOfficialProvider).toHaveBeenCalledTimes(1);
    expect(apiMocks.add).not.toHaveBeenCalled();
    expect(added.provider).toEqual(seedProvider);
    expect(added.alreadyExisted).toBe(true);
    expect(toastMocks.success).not.toHaveBeenCalled();
    expect(toastMocks.info).toHaveBeenCalledTimes(1);
  });

  it("adds a managed Codex account as a separate official card", async () => {
    const { wrapper } = createWrapper();
    const { result } = renderHook(() => useAddProviderMutation("codex"), {
      wrapper,
    });

    const added = await act(async () =>
      result.current.mutateAsync({
        name: "OpenAI Official",
        settingsConfig: { auth: {}, config: "" },
        category: "official",
        meta: {
          providerType: "codex_oauth",
          authBinding: {
            source: "managed_account",
            authProvider: "codex_oauth",
            accountId: "acct-managed",
          },
        },
      }),
    );

    expect(apiMocks.getAll).not.toHaveBeenCalled();
    expect(apiMocks.add).toHaveBeenCalledWith(
      expect.objectContaining({
        id: "generated-uuid",
        category: "official",
        meta: {
          providerType: "codex_oauth",
          authBinding: {
            source: "managed_account",
            authProvider: "codex_oauth",
            accountId: "acct-managed",
          },
        },
      }),
      "codex",
      undefined,
    );
    expect(added.provider).toEqual(
      expect.objectContaining({
        id: "generated-uuid",
        meta: expect.objectContaining({
          authBinding: expect.objectContaining({
            accountId: "acct-managed",
          }),
        }),
      }),
    );
    expect(added.alreadyExisted).toBe(false);
  });

  it("adds every unbound Codex Official as an independent provider", async () => {
    uuidMocks.generateUUID
      .mockReset()
      .mockReturnValueOnce("unbound-official-1")
      .mockReturnValueOnce("unbound-official-2");
    const { wrapper } = createWrapper();
    const { result } = renderHook(() => useAddProviderMutation("codex"), {
      wrapper,
    });

    const first = await act(async () =>
      result.current.mutateAsync({
        name: "OpenAI Official 1",
        settingsConfig: { auth: {}, config: "" },
        category: "official",
        meta: { providerType: "codex_oauth" },
      }),
    );
    const second = await act(async () =>
      result.current.mutateAsync({
        name: "OpenAI Official 2",
        settingsConfig: { auth: {}, config: "" },
        category: "official",
        meta: { providerType: "codex_oauth" },
      }),
    );

    expect(apiMocks.getAll).not.toHaveBeenCalled();
    expect(apiMocks.add).toHaveBeenNthCalledWith(
      1,
      expect.objectContaining({
        id: "unbound-official-1",
        meta: { providerType: "codex_oauth" },
      }),
      "codex",
      undefined,
    );
    expect(apiMocks.add).toHaveBeenNthCalledWith(
      2,
      expect.objectContaining({
        id: "unbound-official-2",
        meta: { providerType: "codex_oauth" },
      }),
      "codex",
      undefined,
    );
    expect(first.provider.id).toBe("unbound-official-1");
    expect(second.provider.id).toBe("unbound-official-2");
  });

  it("adds a Pi provider without a separate default-model command", async () => {
    const { wrapper } = createWrapper();
    const { result } = renderHook(() => useAddProviderMutation("pi"), {
      wrapper,
    });

    const added = await act(async () =>
      result.current.mutateAsync({
        name: "Pi Provider",
        providerKey: "pi-provider",
        settingsConfig: {
          api: "openai-responses",
          baseUrl: "https://example.com/v1",
          apiKey: "secret",
          models: [{ id: "model-a" }],
        },
      }),
    );

    expect(apiMocks.add).toHaveBeenCalledWith(
      expect.objectContaining({ id: "pi-provider" }),
      "pi",
      undefined,
    );
    expect(added.provider.id).toBe("pi-provider");
  });

  it("reports a Pi provider add failure", async () => {
    apiMocks.add.mockRejectedValueOnce(new Error("provider add failed"));
    const { wrapper } = createWrapper();
    const { result } = renderHook(() => useAddProviderMutation("pi"), {
      wrapper,
    });

    await act(async () => {
      await expect(
        result.current.mutateAsync({
          name: "Pi Provider",
          providerKey: "pi-provider",
          settingsConfig: { models: [{ id: "model-a" }] },
        }),
      ).rejects.toThrow("provider add failed");
    });

    expect(apiMocks.add).toHaveBeenCalledWith(
      expect.objectContaining({ id: "pi-provider" }),
      "pi",
      undefined,
    );
    expect(toastMocks.error).toHaveBeenCalled();
    expect(toastMocks.warning).not.toHaveBeenCalled();
  });
});
