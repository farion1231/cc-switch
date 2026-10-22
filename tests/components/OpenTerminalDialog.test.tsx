import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { OpenTerminalDialog } from "@/components/providers/OpenTerminalDialog";
import type { Provider } from "@/types";

const openTerminalMock = vi.fn();
const pickDirectoryMock = vi.fn();
const agyStatusMock = vi.fn();
const agyRunningMock = vi.fn();

vi.mock("@/lib/api", () => ({
  providersApi: {
    openTerminal: (...args: unknown[]) => openTerminalMock(...args),
  },
  settingsApi: {
    pickDirectory: (...args: unknown[]) => pickDirectoryMock(...args),
  },
  authApi: {
    authGetStatus: (...args: unknown[]) => agyStatusMock(...args),
    agyAccountIsRunning: () => agyRunningMock(),
  },
}));
vi.mock("@/lib/platform", () => ({ isMac: () => true }));

vi.mock("@/lib/toast", () => ({
  toast: { success: vi.fn(), error: vi.fn() },
}));

const STORAGE_KEY = "cc-switch:terminal:recent-dirs";
const provider = {
  id: "p1",
  name: "Relay",
  settingsConfig: {
    env: {
      ANTHROPIC_BASE_URL: "https://relay.example.com",
      ANTHROPIC_AUTH_TOKEN: "sk-test",
    },
  },
} as Provider;

function renderDialog(
  onClose = vi.fn(),
  target: Provider = provider,
  appId: "claude" | "agy" = "claude",
) {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  render(
    <QueryClientProvider client={client}>
      <OpenTerminalDialog provider={target} appId={appId} onClose={onClose} />
    </QueryClientProvider>,
  );
  return onClose;
}

describe("OpenTerminalDialog", () => {
  beforeEach(() => {
    window.localStorage.clear();
    openTerminalMock.mockReset().mockResolvedValue(true);
    pickDirectoryMock.mockReset();
    agyStatusMock.mockReset().mockResolvedValue({
      provider: "agy_oauth",
      authenticated: true,
      default_account_id: "a",
      accounts: [
        { id: "a", login: "a@example.com" },
        { id: "b", login: "b@example.com" },
      ],
    });
    agyRunningMock.mockReset().mockResolvedValue(false);
  });

  const agyOfficial = (accountId?: string) =>
    ({
      id: "agy-official",
      name: "Google Official",
      category: "official",
      settingsConfig: { env: {} },
      meta: accountId
        ? {
            authBinding: {
              source: "managed_account",
              authProvider: "agy_oauth",
              accountId,
            },
          }
        : undefined,
    }) as Provider;

  it("says agy's sign-in will switch when the card is bound to another account", async () => {
    agyRunningMock.mockResolvedValue(true);
    renderDialog(vi.fn(), agyOfficial("b"), "agy");
    expect(
      await screen.findByText(/打开前把 agy 的登录切换到 b@example.com/),
    ).toBeInTheDocument();
    expect(await screen.findByText(/检测到 agy 正在运行/)).toBeInTheDocument();
  });

  it("stays quiet when the bound account is already signed in, or nothing is bound", async () => {
    renderDialog(vi.fn(), agyOfficial("a"), "agy");
    await waitFor(() => expect(agyStatusMock).toHaveBeenCalled());
    expect(screen.queryByText(/切换到/)).not.toBeInTheDocument();
    agyStatusMock.mockClear();
    renderDialog(vi.fn(), agyOfficial(), "agy");
    expect(agyStatusMock).not.toHaveBeenCalled();
  });

  it("explains first and only opens the folder picker on request", async () => {
    pickDirectoryMock.mockResolvedValue("/work/alpha");
    const onClose = renderDialog();

    expect(pickDirectoryMock).not.toHaveBeenCalled();
    const confirm = screen.getByRole("button", {
      name: "provider.openTerminalDialog.confirm",
    });
    expect(confirm).toBeDisabled();

    fireEvent.click(
      screen.getByRole("button", {
        name: "provider.openTerminalDialog.choose",
      }),
    );
    await waitFor(() => expect(confirm).toBeEnabled());
    fireEvent.click(confirm);

    await waitFor(() => expect(onClose).toHaveBeenCalled());
    expect(openTerminalMock).toHaveBeenCalledWith("p1", "claude", {
      cwd: "/work/alpha",
    });
    expect(JSON.parse(window.localStorage.getItem(STORAGE_KEY)!)).toEqual([
      "/work/alpha",
    ]);
  });

  it("prefills the last folder and moves a picked recent one to the front", async () => {
    window.localStorage.setItem(
      STORAGE_KEY,
      JSON.stringify(["/work/alpha", "/work/beta"]),
    );
    const onClose = renderDialog();

    // 上次的目录直接填好，另一个在「最近用过」里
    expect(screen.getByTitle("/work/alpha")).toBeInTheDocument();
    fireEvent.click(screen.getByTitle("/work/beta"));
    fireEvent.click(
      screen.getByRole("button", {
        name: "provider.openTerminalDialog.confirm",
      }),
    );

    await waitFor(() => expect(onClose).toHaveBeenCalled());
    expect(openTerminalMock).toHaveBeenCalledWith("p1", "claude", {
      cwd: "/work/beta",
    });
    expect(JSON.parse(window.localStorage.getItem(STORAGE_KEY)!)).toEqual([
      "/work/beta",
      "/work/alpha",
    ]);
  });

  it("keeps the dialog open and the list unchanged when opening fails", async () => {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(["/gone"]));
    openTerminalMock.mockRejectedValue(new Error("目录不存在: /gone"));
    const onClose = renderDialog();

    fireEvent.click(
      screen.getByRole("button", {
        name: "provider.openTerminalDialog.confirm",
      }),
    );

    await waitFor(() => expect(openTerminalMock).toHaveBeenCalled());
    expect(onClose).not.toHaveBeenCalled();
    expect(JSON.parse(window.localStorage.getItem(STORAGE_KEY)!)).toEqual([
      "/gone",
    ]);
  });

  it("shows the isolation notice only when the card carries its own address and key", () => {
    renderDialog();
    expect(
      screen.getByText("provider.openTerminalDialog.isolationTitle"),
    ).toBeInTheDocument();
  });

  // 官方登录卡 env 为空：终端跟着 ~/.claude/settings.json 和全局登录走，不能说隔离
  it("hides the isolation notice for an official login card", () => {
    renderDialog(vi.fn(), {
      id: "official",
      name: "Claude Official",
      settingsConfig: { env: {} },
    } as Provider);
    expect(
      screen.queryByText("provider.openTerminalDialog.isolationTitle"),
    ).not.toBeInTheDocument();
  });

  // agy 带 Key 的卡用自己的私有目录启动；官方登录卡跟着全局登录走
  it("treats only Antigravity key cards as isolated", () => {
    renderDialog(
      vi.fn(),
      {
        id: "relay",
        name: "Relay",
        settingsConfig: { env: { GEMINI_API_KEY: "k" } },
      } as Provider,
      "agy",
    );
    expect(
      screen.getByText("provider.openTerminalDialog.isolationTitle"),
    ).toBeInTheDocument();
  });

  it("hides the isolation notice for the Antigravity login card", () => {
    renderDialog(
      vi.fn(),
      {
        id: "login",
        name: "Google",
        category: "official",
        settingsConfig: { env: {}, config: { model: { name: "m" } } },
      } as Provider,
      "agy",
    );
    expect(
      screen.queryByText("provider.openTerminalDialog.isolationTitle"),
    ).not.toBeInTheDocument();
  });
});
