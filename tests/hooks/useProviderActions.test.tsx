import type { ReactNode } from "react";
import { renderHook, act } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { useProviderActions } from "@/hooks/useProviderActions";
import type { Provider, UsageScript } from "@/types";

const toastSuccessMock = vi.fn();
const toastErrorMock = vi.fn();

vi.mock("sonner", () => ({
  toast: {
    success: (...args: unknown[]) => toastSuccessMock(...args),
    error: (...args: unknown[]) => toastErrorMock(...args),
  },
}));

const addProviderMutateAsync = vi.fn();
const updateProviderMutateAsync = vi.fn();
const deleteProviderMutateAsync = vi.fn();
const switchProviderMutateAsync = vi.fn();

const addProviderMutation = {
  mutateAsync: addProviderMutateAsync,
  isPending: false,
};
const updateProviderMutation = {
  mutateAsync: updateProviderMutateAsync,
  isPending: false,
};
const deleteProviderMutation = {
  mutateAsync: deleteProviderMutateAsync,
  isPending: false,
};
const switchProviderMutation = {
  mutateAsync: switchProviderMutateAsync,
  isPending: false,
};

const useAddProviderMutationMock = vi.fn(() => addProviderMutation);
const useUpdateProviderMutationMock = vi.fn(() => updateProviderMutation);
const useDeleteProviderMutationMock = vi.fn(() => deleteProviderMutation);
const useSwitchProviderMutationMock = vi.fn(() => switchProviderMutation);

vi.mock("@/lib/query", () => ({
  useAddProviderMutation: () => useAddProviderMutationMock(),
  useUpdateProviderMutation: () => useUpdateProviderMutationMock(),
  useDeleteProviderMutation: () => useDeleteProviderMutationMock(),
  useSwitchProviderMutation: () => useSwitchProviderMutationMock(),
}));

const providersApiUpdateMock = vi.fn();
const providersApiUpdateTrayMenuMock = vi.fn();
const settingsApiGetMock = vi.fn();
const settingsApiApplyMock = vi.fn();

vi.mock("@/lib/api", () => ({
  providersApi: {
    update: (...args: unknown[]) => providersApiUpdateMock(...args),
    updateTrayMenu: (...args: unknown[]) =>
      providersApiUpdateTrayMenuMock(...args),
  },
  settingsApi: {
    get: (...args: unknown[]) => settingsApiGetMock(...args),
    applyClaudePluginConfig: (...args: unknown[]) =>
      settingsApiApplyMock(...args),
  },
}));

interface WrapperProps {
  children: ReactNode;
}

function createWrapper() {
  const queryClient = new QueryClient();

  const wrapper = ({ children }: WrapperProps) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );

  return { wrapper, queryClient };
}

function createProvider(overrides: Partial<Provider> = {}): Provider {
  return {
    id: "provider-1",
    name: "Test Provider",
    settingsConfig: {},
    category: "official",
    ...overrides,
  };
}

beforeEach(() => {
  addProviderMutateAsync.mockReset();
  updateProviderMutateAsync.mockReset();
  deleteProviderMutateAsync.mockReset();
  switchProviderMutateAsync.mockReset();
  providersApiUpdateMock.mockReset();
  providersApiUpdateTrayMenuMock.mockReset();
  settingsApiGetMock.mockReset();
  settingsApiApplyMock.mockReset();
  toastSuccessMock.mockReset();
  toastErrorMock.mockReset();

  addProviderMutation.isPending = false;
  updateProviderMutation.isPending = false;
  deleteProviderMutation.isPending = false;
  switchProviderMutation.isPending = false;

  useAddProviderMutationMock.mockClear();
  useUpdateProviderMutationMock.mockClear();
  useDeleteProviderMutationMock.mockClear();
  useSwitchProviderMutationMock.mockClear();
});

describe("useProviderActions", () => {
  it("should trigger mutation when calling addProvider", async () => {
    addProviderMutateAsync.mockResolvedValueOnce(undefined);
    const { wrapper } = createWrapper();
    const providerInput = {
      name: "New Provider",
      settingsConfig: { token: "abc" },
    } as Omit<Provider, "id">;

    const { result } = renderHook(() => useProviderActions("claude"), {
      wrapper,
    });

    await act(async () => {
      await result.current.addProvider(providerInput);
    });

    expect(addProviderMutateAsync).toHaveBeenCalledTimes(1);
    expect(addProviderMutateAsync).toHaveBeenCalledWith(providerInput);
  });

  it("should update tray menu when calling updateProvider", async () => {
    updateProviderMutateAsync.mockResolvedValueOnce(undefined);
    providersApiUpdateTrayMenuMock.mockResolvedValueOnce(true);
    const { wrapper } = createWrapper();
    const provider = createProvider();

    const { result } = renderHook(() => useProviderActions("claude"), {
      wrapper,
    });

    await act(async () => {
      await result.current.updateProvider(provider);
    });

    expect(updateProviderMutateAsync).toHaveBeenCalledWith(provider);
    expect(providersApiUpdateTrayMenuMock).toHaveBeenCalledTimes(1);
  });

  it("should not request plugin sync when switching non-Claude provider", async () => {
    switchProviderMutateAsync.mockResolvedValueOnce(undefined);
    const { wrapper } = createWrapper();
    const provider = createProvider({ category: "custom" });

    const { result } = renderHook(() => useProviderActions("codex"), {
      wrapper,
    });

    await act(async () => {
      await result.current.switchProvider(provider);
    });

    expect(switchProviderMutateAsync).toHaveBeenCalledWith(provider.id);
    expect(settingsApiGetMock).not.toHaveBeenCalled();
    expect(settingsApiApplyMock).not.toHaveBeenCalled();
  });

  it("should sync plugin config when switching Claude provider with integration enabled", async () => {
    switchProviderMutateAsync.mockResolvedValueOnce(undefined);
    settingsApiGetMock.mockResolvedValueOnce({
      enableClaudePluginIntegration: true,
    });
    settingsApiApplyMock.mockResolvedValueOnce(true);
    const { wrapper } = createWrapper();
    const provider = createProvider({ category: "official" });

    const { result } = renderHook(() => useProviderActions("claude"), {
      wrapper,
    });

    await act(async () => {
      await result.current.switchProvider(provider);
    });

    expect(switchProviderMutateAsync).toHaveBeenCalledWith(provider.id);
    expect(settingsApiGetMock).toHaveBeenCalledTimes(1);
    expect(settingsApiApplyMock).toHaveBeenCalledWith({ official: true });
  });

  it("should not call applyClaudePluginConfig when integration is disabled", async () => {
    switchProviderMutateAsync.mockResolvedValueOnce(undefined);
    settingsApiGetMock.mockResolvedValueOnce({
      enableClaudePluginIntegration: false,
    });
    const { wrapper } = createWrapper();
    const provider = createProvider();

    const { result } = renderHook(() => useProviderActions("claude"), {
      wrapper,
    });

    await act(async () => {
      await result.current.switchProvider(provider);
    });

    expect(settingsApiGetMock).toHaveBeenCalledTimes(1);
    expect(settingsApiApplyMock).not.toHaveBeenCalled();
  });

  it("should show error toast when plugin sync fails with error message", async () => {
    switchProviderMutateAsync.mockResolvedValueOnce(undefined);
    settingsApiGetMock.mockResolvedValueOnce({
      enableClaudePluginIntegration: true,
    });
    settingsApiApplyMock.mockRejectedValueOnce(new Error("Sync failed"));
    const { wrapper } = createWrapper();
    const provider = createProvider();

    const { result } = renderHook(() => useProviderActions("claude"), {
      wrapper,
    });

    await act(async () => {
      await result.current.switchProvider(provider);
    });

    expect(toastErrorMock).toHaveBeenCalledTimes(1);
    expect(toastErrorMock.mock.calls[0]?.[0]).toBe("Sync failed");
  });

  it("propagates updateProvider errors", async () => {
    updateProviderMutateAsync.mockRejectedValueOnce(new Error("update failed"));
    const { wrapper } = createWrapper();
    const provider = createProvider();

    const { result } = renderHook(() => useProviderActions("claude"), {
      wrapper,
    });

    await expect(
      act(async () => {
        await result.current.updateProvider(provider);
      }),
    ).rejects.toThrow("update failed");
  });

  it("should use default error message when plugin sync fails without error message", async () => {
    switchProviderMutateAsync.mockResolvedValueOnce(undefined);
    settingsApiGetMock.mockResolvedValueOnce({
      enableClaudePluginIntegration: true,
    });
    settingsApiApplyMock.mockRejectedValueOnce(new Error(""));
    const { wrapper } = createWrapper();
    const provider = createProvider();

    const { result } = renderHook(() => useProviderActions("claude"), {
      wrapper,
    });

    await act(async () => {
      await result.current.switchProvider(provider);
    });

    expect(toastErrorMock).toHaveBeenCalledTimes(1);
    expect(toastErrorMock.mock.calls[0]?.[0]).toBe("同步 Claude 插件失败");
  });

  it("handles mutation errors when plugin sync is skipped", async () => {
    switchProviderMutateAsync.mockRejectedValueOnce(new Error("switch failed"));
    const { wrapper } = createWrapper();
    const provider = createProvider();

    const { result } = renderHook(() => useProviderActions("codex"), {
      wrapper,
    });

    await expect(
      result.current.switchProvider(provider),
    ).resolves.toBeUndefined();
    expect(settingsApiGetMock).not.toHaveBeenCalled();
    expect(settingsApiApplyMock).not.toHaveBeenCalled();
  });

  it("should call delete mutation when calling deleteProvider", async () => {
    deleteProviderMutateAsync.mockResolvedValueOnce(undefined);
    const { wrapper } = createWrapper();

    const { result } = renderHook(() => useProviderActions("claude"), {
      wrapper,
    });

    await act(async () => {
      await result.current.deleteProvider("provider-2");
    });

    expect(deleteProviderMutateAsync).toHaveBeenCalledWith("provider-2");
  });

  it("should update provider and refresh cache when saveUsageScript succeeds", async () => {
    providersApiUpdateMock.mockResolvedValueOnce(true);
    const { wrapper, queryClient } = createWrapper();
    const invalidateSpy = vi.spyOn(queryClient, "invalidateQueries");

    const provider = createProvider({
      meta: {
        usage_script: {
          enabled: false,
          language: "javascript",
          code: "",
        },
      },
    });

    const script: UsageScript = {
      enabled: true,
      language: "javascript",
      code: "return { success: true };",
      timeout: 5,
    };

    const { result } = renderHook(() => useProviderActions("claude"), {
      wrapper,
    });

    await act(async () => {
      await result.current.saveUsageScript(provider, script);
    });

    expect(providersApiUpdateMock).toHaveBeenCalledWith(
      {
        ...provider,
        meta: {
          ...provider.meta,
          usage_script: script,
        },
      },
      "claude",
    );
    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: ["providers", "claude"],
    });
    expect(toastSuccessMock).toHaveBeenCalledTimes(1);
  });

  it("should show error toast when saveUsageScript fails with error message", async () => {
    providersApiUpdateMock.mockRejectedValueOnce(new Error("Save failed"));
    const { wrapper } = createWrapper();
    const provider = createProvider();
    const script: UsageScript = {
      enabled: true,
      language: "javascript",
      code: "return {}",
    };

    const { result } = renderHook(() => useProviderActions("claude"), {
      wrapper,
    });

    await act(async () => {
      await result.current.saveUsageScript(provider, script);
    });

    expect(toastErrorMock).toHaveBeenCalledTimes(1);
    expect(toastErrorMock.mock.calls[0]?.[0]).toBe("Save failed");
  });

  it("should use default error message when saveUsageScript fails without error message", async () => {
    providersApiUpdateMock.mockRejectedValueOnce(new Error(""));
    const { wrapper } = createWrapper();
    const provider = createProvider();
    const script: UsageScript = {
      enabled: true,
      language: "javascript",
      code: "return {}",
    };

    const { result } = renderHook(() => useProviderActions("claude"), {
      wrapper,
    });

    await act(async () => {
      await result.current.saveUsageScript(provider, script);
    });

    expect(toastErrorMock).toHaveBeenCalledTimes(1);
    expect(toastErrorMock.mock.calls[0]?.[0]).toBe("用量查询配置保存失败");
  });

  it("propagates addProvider errors to caller", async () => {
    addProviderMutateAsync.mockRejectedValueOnce(new Error("add failed"));
    const { wrapper } = createWrapper();

    const { result } = renderHook(() => useProviderActions("claude"), {
      wrapper,
    });

    await expect(
      act(async () => {
        await result.current.addProvider({
          name: "temp",
          settingsConfig: {},
        } as Omit<Provider, "id">);
      }),
    ).rejects.toThrow("add failed");
  });

  it("propagates deleteProvider errors to caller", async () => {
    deleteProviderMutateAsync.mockRejectedValueOnce(new Error("delete failed"));
    const { wrapper } = createWrapper();

    const { result } = renderHook(() => useProviderActions("claude"), {
      wrapper,
    });

    await expect(
      act(async () => {
        await result.current.deleteProvider("provider-2");
      }),
    ).rejects.toThrow("delete failed");
  });

  it("handles switch mutation errors silently", async () => {
    switchProviderMutateAsync.mockRejectedValueOnce(new Error("switch failed"));
    const { wrapper } = createWrapper();
    const provider = createProvider();

    const { result } = renderHook(() => useProviderActions("claude"), {
      wrapper,
    });

    await result.current.switchProvider(provider);

    expect(settingsApiGetMock).not.toHaveBeenCalled();
    expect(settingsApiApplyMock).not.toHaveBeenCalled();
  });

  it("should track pending state of all mutations in isLoading", () => {
    addProviderMutation.isPending = true;
    const { wrapper } = createWrapper();

    const { result } = renderHook(() => useProviderActions("claude"), {
      wrapper,
    });

    expect(result.current.isLoading).toBe(true);
  });
});
it("clears loading flag when all mutations idle", () => {
  addProviderMutation.isPending = false;
  updateProviderMutation.isPending = false;
  deleteProviderMutation.isPending = false;
  switchProviderMutation.isPending = false;

  const { wrapper } = createWrapper();
  const { result } = renderHook(() => useProviderActions("claude"), {
    wrapper,
  });

  expect(result.current.isLoading).toBe(false);
});
