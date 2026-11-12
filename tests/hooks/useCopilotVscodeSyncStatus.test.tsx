import { renderHook, waitFor, act } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ReactNode } from "react";
import { useCopilotVscodeSyncNotifications } from "@/hooks/useCopilotVscodeSyncStatus";
import { copilotVscodeApi } from "@/lib/api/copilotVscode";
import { toast } from "@/lib/toast";

vi.mock("@/lib/api/copilotVscode", () => ({
  copilotVscodeStatusKey: ["copilot-vscode-sync-status"],
  copilotVscodeApi: { status: vi.fn(), sync: vi.fn() },
}));
vi.mock("@/lib/toast", () => ({
  toast: { warning: vi.fn(), dismiss: vi.fn(), error: vi.fn() },
}));

describe("Copilot CLI save notifications", () => {
  beforeEach(() => {
    vi.mocked(copilotVscodeApi.sync).mockReset().mockResolvedValue(undefined);
    vi.mocked(copilotVscodeApi.status)
      .mockReset()
      .mockResolvedValue({ pending: false, error: null });
  });

  function mount(enabled: boolean) {
    const client = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const wrapper = ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={client}>{children}</QueryClientProvider>
    );
    const hook = renderHook(
      () =>
        useCopilotVscodeSyncNotifications({
          enabled,
          targets: ["/profile/chatLanguageModels.json"],
        }),
      { wrapper },
    );
    return { ...hook, client };
  }

  it("leaves disabled integration idle", async () => {
    const { client } = mount(false);
    act(() => client.setQueryData(["providers", "copilot"], {}));
    await act(async () => {});
    expect(copilotVscodeApi.status).not.toHaveBeenCalled();
    expect(toast.warning).not.toHaveBeenCalled();
  });

  it("warns after the saved CLI provider list refreshes and clears after recovery", async () => {
    const { client } = mount(true);
    await waitFor(() =>
      expect(copilotVscodeApi.status).toHaveBeenCalledTimes(1),
    );
    vi.mocked(copilotVscodeApi.status).mockResolvedValue({
      pending: true,
      error: "locked target",
    });
    act(() =>
      client.setQueryData(["providers", "copilot"], { providers: ["saved"] }),
    );
    await waitFor(() =>
      expect(toast.warning).toHaveBeenCalledWith(
        "settings.copilotVscode.pending",
        expect.objectContaining({
          id: "copilot-vscode-sync",
          description: "locked target",
        }),
      ),
    );
    const warnings = vi.mocked(toast.warning).mock.calls.length;
    act(() =>
      client.setQueryData(["providers", "copilot"], {
        providers: ["saved-again"],
      }),
    );
    await waitFor(() =>
      expect(copilotVscodeApi.status).toHaveBeenCalledTimes(3),
    );
    expect(toast.warning).toHaveBeenCalledTimes(warnings);
    vi.mocked(copilotVscodeApi.status).mockResolvedValue({
      pending: false,
      error: null,
    });
    act(() =>
      client.setQueryData(["providers", "copilot"], {
        providers: ["recovered"],
      }),
    );
    await waitFor(() =>
      expect(toast.dismiss).toHaveBeenCalledWith("copilot-vscode-sync"),
    );
  });

  it("offers retry from the failure notification without extra settings controls", async () => {
    vi.mocked(copilotVscodeApi.status).mockResolvedValue({
      pending: true,
      error: "locked target",
    });
    mount(true);
    await waitFor(() => expect(toast.warning).toHaveBeenCalled());
    const action = vi.mocked(toast.warning).mock.calls[0][1]?.action;
    expect(action).toMatchObject({ label: "common.retry" });
    if (!action || typeof action !== "object" || !("onClick" in action))
      throw new Error("Missing retry action");
    vi.mocked(copilotVscodeApi.sync).mockRejectedValueOnce(
      new Error("still locked"),
    );
    await act(async () => action.onClick({} as never));
    expect(toast.error).toHaveBeenCalledWith(
      "settings.copilotVscode.failed",
      expect.objectContaining({
        id: "copilot-vscode-sync",
        description: "still locked",
        action: expect.objectContaining({ label: "common.retry" }),
      }),
    );
    const retry = vi.mocked(toast.error).mock.calls[0][1]?.action;
    if (!retry || typeof retry !== "object" || !("onClick" in retry))
      throw new Error("Missing retry action");
    vi.mocked(copilotVscodeApi.status).mockResolvedValue({
      pending: false,
      error: null,
    });
    await act(async () => retry.onClick({} as never));
    expect(copilotVscodeApi.sync).toHaveBeenCalledTimes(2);
    await waitFor(() =>
      expect(toast.dismiss).toHaveBeenCalledWith("copilot-vscode-sync"),
    );
  });
});
