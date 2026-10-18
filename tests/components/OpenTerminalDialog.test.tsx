import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { OpenTerminalDialog } from "@/components/providers/OpenTerminalDialog";
import type { Provider } from "@/types";

const openTerminalMock = vi.fn();
const pickDirectoryMock = vi.fn();

vi.mock("@/lib/api", () => ({
  providersApi: {
    openTerminal: (...args: unknown[]) => openTerminalMock(...args),
  },
  settingsApi: {
    pickDirectory: (...args: unknown[]) => pickDirectoryMock(...args),
  },
}));

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

function renderDialog(onClose = vi.fn(), target: Provider = provider) {
  render(
    <OpenTerminalDialog provider={target} appId="claude" onClose={onClose} />,
  );
  return onClose;
}

describe("OpenTerminalDialog", () => {
  beforeEach(() => {
    window.localStorage.clear();
    openTerminalMock.mockReset().mockResolvedValue(true);
    pickDirectoryMock.mockReset();
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
});
