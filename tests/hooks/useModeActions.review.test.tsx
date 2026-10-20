import type { ReactNode } from "react";
import { act, renderHook } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useModeActions } from "@/hooks/useModeActions";

const mocks = vi.hoisted(() => ({
  review: vi.fn(),
  apply: vi.fn(),
  takeover: vi.fn(),
  switch: vi.fn(),
  tray: vi.fn(),
}));
vi.mock("@/components/providers/CodexSwitchReview", () => ({
  useCodexSwitchReview: () => ({ review: mocks.review, dialog: null }),
}));
vi.mock("@/lib/api/codexSelection", () => ({
  codexSelectionApi: { apply: mocks.apply },
}));
vi.mock("@/lib/api/proxy", () => ({
  proxyApi: { setProxyTakeoverForApp: mocks.takeover },
}));
vi.mock("@/lib/api/providers", () => ({
  providersApi: { switch: mocks.switch, updateTrayMenu: mocks.tray },
}));
vi.mock("@/lib/toast", () => ({ toast: { success: vi.fn() } }));

function wrapper({ children }: { children: ReactNode }) {
  return (
    <QueryClientProvider
      client={
        new QueryClient({ defaultOptions: { queries: { retry: false } } })
      }
    >
      {children}
    </QueryClientProvider>
  );
}
const args = {
  directProviderId: "old",
  useProviderId: "imported",
  providerName: "Imported",
  reviewCodex: true,
};
describe("reviewed Codex exit to direct", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.tray.mockResolvedValue(undefined);
    mocks.apply.mockResolvedValue({});
    mocks.takeover.mockResolvedValue(undefined);
    mocks.switch.mockResolvedValue({});
  });
  it("cancelling keeps both mode and configuration unchanged", async () => {
    mocks.review.mockResolvedValue(null);
    const { result } = renderHook(() => useModeActions("codex"), { wrapper });
    await act(() => result.current.exitToDirect(args));
    expect(mocks.review).toHaveBeenCalledWith("imported", true);
    expect(mocks.apply).not.toHaveBeenCalled();
    expect(mocks.takeover).not.toHaveBeenCalled();
    expect(mocks.switch).not.toHaveBeenCalled();
  });
  it("commits the reviewed exit and switch as one backend operation", async () => {
    const selection = {
      revision: "snapshot",
      selected: ["connection"],
      exitToDirect: true,
    };
    mocks.review.mockResolvedValue(selection);
    const { result } = renderHook(() => useModeActions("codex"), { wrapper });
    await act(() => result.current.exitToDirect(args));
    expect(mocks.apply).toHaveBeenCalledWith("imported", selection);
    expect(mocks.takeover).not.toHaveBeenCalled();
    expect(mocks.switch).not.toHaveBeenCalled();
  });
  it("retains the upstream flow for other clients", async () => {
    const { result } = renderHook(() => useModeActions("claude"), { wrapper });
    await act(() => result.current.exitToDirect(args));
    expect(mocks.review).not.toHaveBeenCalled();
    expect(mocks.takeover).toHaveBeenCalledWith("claude", false);
    expect(mocks.switch).toHaveBeenCalledWith("imported", "claude");
  });
});
