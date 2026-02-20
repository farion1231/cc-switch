import { renderHook, act, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, it, expect, vi, beforeEach } from "vitest";
import type { ReactNode } from "react";
import { usePluginList, useSetPluginEnabled } from "@/hooks/usePlugins";

const listMock = vi.fn();
const setEnabledMock = vi.fn();

vi.mock("@/lib/api", () => ({
  pluginsApi: {
    list: (...args: unknown[]) => listMock(...args),
    setEnabled: (...args: unknown[]) => setEnabledMock(...args),
  },
}));

// Mock Tauri event listener（listen 返回 unlisten 函数）
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));

function wrapper({ children }: { children: ReactNode }) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return <QueryClientProvider client={qc}>{children}</QueryClientProvider>;
}

describe("usePluginList", () => {
  beforeEach(() => {
    listMock.mockResolvedValue([
      { plugin_id: "foo@bar", enabled: true, install_path: "/p", scope: "user", version: "1.0" },
    ]);
  });

  it("returns plugin list", async () => {
    const { result } = renderHook(() => usePluginList(), { wrapper });
    await waitFor(() => expect(result.current.data).toHaveLength(1));
    expect(result.current.data?.[0].plugin_id).toBe("foo@bar");
  });
});

describe("useSetPluginEnabled", () => {
  it("calls api.setEnabled", async () => {
    setEnabledMock.mockResolvedValue(true);
    const { result } = renderHook(() => useSetPluginEnabled(), { wrapper });
    await act(async () => {
      await result.current.mutateAsync({ pluginId: "foo@bar", enabled: false });
    });
    expect(setEnabledMock).toHaveBeenCalledWith("foo@bar", false);
  });
});
