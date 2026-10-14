import { renderHook, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ReactNode } from "react";
import { useCodexDesktopStatus } from "@/hooks/useCodexDesktopStatus";

const mocks = vi.hoisted(() => ({ status: vi.fn(), windows: vi.fn() }));
vi.mock("@/lib/api/codexDesktop", () => ({
  getCodexDesktopStatus: mocks.status,
}));
vi.mock("@/lib/platform", () => ({ isWindows: mocks.windows }));

function wrapper({ children }: { children: ReactNode }) {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return <QueryClientProvider client={client}>{children}</QueryClientProvider>;
}

describe("useCodexDesktopStatus", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.windows.mockReturnValue(true);
    mocks.status.mockResolvedValue({
      supported: true,
      installed: true,
      running: false,
    });
  });
  it("reads actual desktop state on Windows", async () => {
    const { result } = renderHook(() => useCodexDesktopStatus(), { wrapper });
    await waitFor(() => expect(result.current.data?.installed).toBe(true));
    expect(result.current.data?.running).toBe(false);
    expect(mocks.status).toHaveBeenCalledTimes(1);
  });
  it("does not query Windows package discovery on macOS", () => {
    mocks.windows.mockReturnValue(false);
    renderHook(() => useCodexDesktopStatus(), { wrapper });
    expect(mocks.status).not.toHaveBeenCalled();
  });
  it("does not query when the Codex provider page is inactive", () => {
    renderHook(() => useCodexDesktopStatus(false), { wrapper });
    expect(mocks.status).not.toHaveBeenCalled();
  });
});
