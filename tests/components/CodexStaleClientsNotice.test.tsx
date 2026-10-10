import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { http, HttpResponse } from "msw";
import { toast } from "sonner";
import { CodexStaleClientsNotice } from "@/components/providers/CodexStaleClientsNotice";
import type { CodexStaleClients } from "@/types/proxy";
import { server } from "../msw/server";

vi.mock("sonner", () => ({
  toast: {
    success: vi.fn(),
    info: vi.fn(),
    error: vi.fn(),
    warning: vi.fn(),
  },
}));

const TAURI_ENDPOINT = "http://tauri.local";
const RESTART = `${TAURI_ENDPOINT}/restart_codex_app_server_daemon`;
const ACKNOWLEDGE = `${TAURI_ENDPOINT}/acknowledge_codex_stale_clients`;

function renderNotice(staleClients: CodexStaleClients) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  const invalidate = vi.spyOn(queryClient, "invalidateQueries");
  render(
    <QueryClientProvider client={queryClient}>
      <CodexStaleClientsNotice staleClients={staleClients} />
    </QueryClientProvider>,
  );
  return { invalidate };
}

function clickRestartAndConfirm() {
  fireEvent.click(
    screen.getByRole("button", { name: "proxy.stackMode.codexStale.restart" }),
  );
  fireEvent.click(
    screen.getByRole("button", { name: "proxy.stackMode.codexStale.confirm" }),
  );
}

describe("CodexStaleClientsNotice", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("explains a potentially cached account without claiming models are missing", () => {
    renderNotice({ daemon: true, others: false, auth: true });
    expect(
      screen.getByText("proxy.stackMode.codexStale.authTitle"),
    ).toBeInTheDocument();
    expect(
      screen.queryByText("proxy.stackMode.codexStale.title"),
    ).not.toBeInTheDocument();
  });

  it("says Codex may be stale when its processes cannot be seen and records the dismissal", async () => {
    let acknowledged = 0;
    server.use(
      http.post(ACKNOWLEDGE, () => {
        acknowledged += 1;
        return HttpResponse.json(null);
      }),
    );
    const { invalidate } = renderNotice({
      daemon: false,
      others: true,
      auth: true,
      unverified: true,
    });
    expect(
      screen.getByText("proxy.stackMode.codexStale.unverifiedTitle"),
    ).toBeInTheDocument();
    expect(
      screen.queryByText("proxy.stackMode.codexStale.authTitle"),
    ).not.toBeInTheDocument();
    expect(
      screen.getByText("proxy.stackMode.codexStale.unverifiedHint"),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "proxy.stackMode.codexStale.restart" }),
    ).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "common.close" }));
    await waitFor(() => expect(acknowledged).toBe(1));
    expect(invalidate).toHaveBeenCalledWith({ queryKey: ["providers", "codex"] });
  });

  it("does not record a dismissal for a notice backed by the process table", () => {
    let acknowledged = 0;
    server.use(
      http.post(ACKNOWLEDGE, () => {
        acknowledged += 1;
        return HttpResponse.json(null);
      }),
    );
    renderNotice({ daemon: false, others: true });
    // 没有 onDismiss 也不是「可能」：不给关闭按钮，也不记确认。
    expect(
      screen.queryByRole("button", { name: "common.close" }),
    ).not.toBeInTheDocument();
    expect(acknowledged).toBe(0);
  });

  it("restarts the daemon only after the user confirms", async () => {
    let restarts = 0;
    server.use(
      http.post(RESTART, () => {
        restarts += 1;
        return HttpResponse.json("restarted");
      }),
    );
    const { invalidate } = renderNotice({ daemon: true, others: false });

    expect(
      screen.getByText("proxy.stackMode.codexStale.daemon"),
    ).toBeInTheDocument();
    expect(
      screen.queryByText("proxy.stackMode.codexStale.others"),
    ).not.toBeInTheDocument();

    fireEvent.click(
      screen.getByRole("button", { name: "proxy.stackMode.codexStale.restart" }),
    );
    // 只是打开确认框：会中断正在运行的任务，没确认之前不重启。
    expect(
      screen.getByText("proxy.stackMode.codexStale.confirmMessage"),
    ).toBeInTheDocument();
    expect(restarts).toBe(0);

    fireEvent.click(
      screen.getByRole("button", { name: "proxy.stackMode.codexStale.confirm" }),
    );
    await waitFor(() =>
      expect(toast.success).toHaveBeenCalledWith(
        "proxy.stackMode.codexStale.restarted",
        expect.anything(),
      ),
    );
    expect(restarts).toBe(1);
    // 重新查 Stack 名单，提示随之消失。
    expect(invalidate).toHaveBeenCalledWith({ queryKey: ["providers", "codex"] });
    await waitFor(() =>
      expect(
        screen.queryByText("proxy.stackMode.codexStale.confirmMessage"),
      ).not.toBeInTheDocument(),
    );
  });

  it("says so when the daemon is not running", async () => {
    server.use(http.post(RESTART, () => HttpResponse.json("notRunning")));
    renderNotice({ daemon: true, others: true });
    clickRestartAndConfirm();
    await waitFor(() =>
      expect(toast.info).toHaveBeenCalledWith(
        "proxy.stackMode.codexStale.notRunning",
        expect.anything(),
      ),
    );
    expect(toast.success).not.toHaveBeenCalled();
  });

  it("shows the failure", async () => {
    server.use(
      http.post(RESTART, () =>
        HttpResponse.json("重启 Codex 守护进程失败: boom", { status: 500 }),
      ),
    );
    renderNotice({ daemon: true, others: false });
    clickRestartAndConfirm();
    await waitFor(() =>
      expect(toast.error).toHaveBeenCalledWith(
        "proxy.stackMode.codexStale.failed",
      ),
    );
  });

  it("only explains how to reopen the desktop app", () => {
    renderNotice({ daemon: false, others: true });
    expect(
      screen.getByText("proxy.stackMode.codexStale.others"),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", {
        name: "proxy.stackMode.codexStale.restart",
      }),
    ).not.toBeInTheDocument();
  });
});
