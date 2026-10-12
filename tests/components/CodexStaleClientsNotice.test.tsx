import { StrictMode } from "react";
import {
  act,
  render,
  screen,
  fireEvent,
  cleanup,
} from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { toast, type Action } from "sonner";
import { CodexStaleClientsNotice } from "@/components/providers/CodexStaleClientsNotice";
import { Toaster } from "@/components/ui/sonner";
import { proxyApi } from "@/lib/api/proxy";
import type { CodexStaleClients } from "@/types/proxy";

vi.mock("@/components/theme-provider", () => ({
  useTheme: () => ({ theme: "light" }),
}));

const title = "proxy.stackMode.codexStale.title";
const restartLabel = "proxy.stackMode.codexStale.restart";
const clients = { daemon: true, others: false };

function renderNotice(
  staleClients: CodexStaleClients = clients,
  queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: 3 } },
  }),
) {
  const onDismiss = vi.fn();
  const invalidate = vi.spyOn(queryClient, "invalidateQueries");
  const content = (dismiss = onDismiss) => (
    <StrictMode>
      <QueryClientProvider client={queryClient}>
        <CodexStaleClientsNotice
          staleClients={staleClients}
          onDismiss={dismiss}
        />
        <Toaster />
      </QueryClientProvider>
    </StrictMode>
  );
  const view = render(content());
  return { ...view, content, invalidate, onDismiss, queryClient };
}

async function advance(ms: number) {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(ms);
  });
}

describe("CodexStaleClientsNotice with the real Sonner toaster", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.spyOn(proxyApi, "restartCodexAppServerDaemon").mockResolvedValue(
      "restarted",
    );
    vi.spyOn(proxyApi, "acknowledgeCodexStaleClients").mockResolvedValue(
      undefined,
    );
  });

  afterEach(() => {
    cleanup();
    toast.dismiss();
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it("explains cached accounts and the impact of restarting before any click", async () => {
    renderNotice({ ...clients, auth: true });
    await advance(100);
    expect(
      screen.getByText("proxy.stackMode.codexStale.authTitle"),
    ).toBeInTheDocument();
    expect(
      screen.getByText("proxy.stackMode.codexStale.restartWarning"),
    ).toBeInTheDocument();
    expect(screen.queryByText(title)).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: restartLabel }),
    ).toBeInTheDocument();
    expect(proxyApi.restartCodexAppServerDaemon).not.toHaveBeenCalled();
  });

  it("automatically closes without restarting or acknowledging clients", async () => {
    const { onDismiss } = renderNotice({ ...clients, unverified: true });
    await advance(8500);
    expect(
      screen.queryByRole("button", { name: restartLabel }),
    ).not.toBeInTheDocument();
    expect(onDismiss).toHaveBeenCalledTimes(1);
    expect(proxyApi.restartCodexAppServerDaemon).not.toHaveBeenCalled();
    expect(proxyApi.acknowledgeCodexStaleClients).not.toHaveBeenCalled();
  });

  it("resumes its timeout after hovering and leaving without clicking", async () => {
    const { onDismiss } = renderNotice();
    await advance(2000);
    const notification = screen
      .getByText(title)
      .closest("[data-sonner-toaster]")!;
    fireEvent.mouseEnter(notification);
    await advance(10000);
    expect(screen.getByText(title)).toBeInTheDocument();
    expect(onDismiss).not.toHaveBeenCalled();
    fireEvent.mouseLeave(notification);
    await advance(6500);
    expect(screen.queryByText(title)).not.toBeInTheDocument();
    expect(onDismiss).toHaveBeenCalledTimes(1);
    expect(proxyApi.restartCodexAppServerDaemon).not.toHaveBeenCalled();
  });

  it("closes on a restart click before completion, prevents double clicks, and reports success after unmount", async () => {
    let finish!: (outcome: "restarted") => void;
    vi.mocked(proxyApi.restartCodexAppServerDaemon).mockReturnValue(
      new Promise((resolve) => {
        finish = resolve;
      }),
    );
    const view = renderNotice({ ...clients, unverified: true });
    await advance(100);
    const button = screen.getByRole("button", { name: restartLabel });
    fireEvent.click(button);
    fireEvent.click(button);
    await advance(500);
    expect(view.onDismiss).toHaveBeenCalledTimes(1);
    expect(proxyApi.restartCodexAppServerDaemon).toHaveBeenCalledTimes(1);
    expect(
      screen.queryByRole("button", { name: restartLabel }),
    ).not.toBeInTheDocument();
    expect(proxyApi.acknowledgeCodexStaleClients).not.toHaveBeenCalled();
    view.unmount();
    render(<Toaster />);
    await act(async () => finish("restarted"));
    await advance(100);
    expect(
      screen.getByText("proxy.stackMode.codexStale.restarted"),
    ).toBeInTheDocument();
    expect(view.invalidate).toHaveBeenCalledWith({
      queryKey: ["providers", "codex"],
    });
  });

  it("does not submit another restart from a new notification while one is pending", async () => {
    let finish!: (outcome: "restarted") => void;
    vi.mocked(proxyApi.restartCodexAppServerDaemon).mockReturnValue(
      new Promise((resolve) => {
        finish = resolve;
      }),
    );
    const first = renderNotice();
    await advance(100);
    fireEvent.click(screen.getByRole("button", { name: restartLabel }));
    await advance(500);
    first.unmount();
    const next = renderNotice(clients, first.queryClient);
    await advance(100);
    fireEvent.click(screen.getByRole("button", { name: restartLabel }));
    await advance(100);
    expect(proxyApi.restartCodexAppServerDaemon).toHaveBeenCalledTimes(1);
    expect(next.onDismiss).not.toHaveBeenCalled();
    expect(
      screen.getByText("proxy.stackMode.codexStale.restarting"),
    ).toBeInTheDocument();
    await act(async () => finish("restarted"));
  });

  it("reports a stopped daemon without claiming it was restarted", async () => {
    vi.mocked(proxyApi.restartCodexAppServerDaemon).mockResolvedValue(
      "notRunning",
    );
    renderNotice();
    await advance(100);
    fireEvent.click(screen.getByRole("button", { name: restartLabel }));
    await advance(500);
    expect(
      screen.getByText("proxy.stackMode.codexStale.notRunning"),
    ).toBeInTheDocument();
    expect(
      screen.queryByText("proxy.stackMode.codexStale.restarted"),
    ).not.toBeInTheDocument();
  });

  it("reports failures without automatically retrying a restart", async () => {
    vi.mocked(proxyApi.restartCodexAppServerDaemon).mockRejectedValue(
      new Error("boom"),
    );
    renderNotice();
    await advance(100);
    fireEvent.click(screen.getByRole("button", { name: restartLabel }));
    await advance(500);
    expect(
      screen.getByText("proxy.stackMode.codexStale.failed"),
    ).toBeInTheDocument();
    await advance(10000);
    expect(proxyApi.restartCodexAppServerDaemon).toHaveBeenCalledTimes(1);
  });

  it("acknowledges uncertain clients only on an explicit dismissal", async () => {
    const { onDismiss } = renderNotice({
      daemon: false,
      others: true,
      unverified: true,
    });
    await advance(100);
    expect(
      screen.getByText("proxy.stackMode.codexStale.unverifiedHint"),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: restartLabel }),
    ).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Close toast" }));
    await advance(500);
    expect(onDismiss).toHaveBeenCalledTimes(1);
    expect(proxyApi.acknowledgeCodexStaleClients).toHaveBeenCalledTimes(1);
    expect(proxyApi.restartCodexAppServerDaemon).not.toHaveBeenCalled();
  });

  it("withdraws obsolete actions on unmount without acknowledging a new config", async () => {
    const view = renderNotice({ ...clients, unverified: true });
    await advance(100);
    const action = toast
      .getToasts()
      .flatMap((item) =>
        "action" in item && item.action ? [item.action] : [],
      )[0] as Action;
    expect(action).toBeDefined();
    view.unmount();
    act(() => action.onClick({} as never));
    await advance(500);
    expect(proxyApi.restartCodexAppServerDaemon).not.toHaveBeenCalled();
    expect(proxyApi.acknowledgeCodexStaleClients).not.toHaveBeenCalled();
    expect(view.onDismiss).not.toHaveBeenCalled();
  });

  it("does not reset the timeout on parent renders and uses the current dismissal callback", async () => {
    const view = renderNotice();
    await advance(4000);
    const latestDismiss = vi.fn();
    view.rerender(view.content(latestDismiss));
    await advance(4500);
    expect(screen.queryByText(title)).not.toBeInTheDocument();
    expect(latestDismiss).toHaveBeenCalledTimes(1);
    expect(view.onDismiss).not.toHaveBeenCalled();
  });
});
