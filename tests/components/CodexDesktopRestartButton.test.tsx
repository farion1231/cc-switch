import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { CodexDesktopRestartButton } from "@/components/providers/CodexDesktopRestartButton";

const mocks = vi.hoisted(() => ({
  restart: vi.fn(),
  success: vi.fn(),
  error: vi.fn(),
  status: {
    data: { supported: true, installed: true, running: true },
    isError: false,
    refetch: vi.fn(),
  },
}));
vi.mock("@/lib/api/codexDesktop", () => ({
  restartCodexDesktop: mocks.restart,
}));
vi.mock("@/lib/toast", () => ({
  toast: { success: mocks.success, error: mocks.error },
}));
vi.mock("@/hooks/useCodexDesktopStatus", () => ({
  useCodexDesktopStatus: () => mocks.status,
}));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, values?: { error?: string }) =>
      key + (values?.error ?? ""),
  }),
}));

describe("CodexDesktopRestartButton", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.status.data = { supported: true, installed: true, running: true };
    mocks.status.isError = false;
  });
  it("hides controls when the desktop is not installed", () => {
    mocks.status.data.installed = false;
    render(<CodexDesktopRestartButton />);
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
  });
  it("hides controls on an unsupported platform", () => {
    mocks.status.data.supported = false;
    render(<CodexDesktopRestartButton />);
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
  });
  it("hides controls when detection fails", () => {
    mocks.status.isError = true;
    render(<CodexDesktopRestartButton />);
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
  });
  it("opens an installed desktop that is not running", async () => {
    mocks.status.data.running = false;
    mocks.restart.mockResolvedValue({ action: "started", desktop_pid: 42 });
    render(<CodexDesktopRestartButton />);
    await userEvent.click(
      screen.getByRole("button", { name: "codexDesktop.open" }),
    );
    await waitFor(() =>
      expect(mocks.success).toHaveBeenCalledWith("codexDesktop.opened"),
    );
    expect(mocks.status.refetch).toHaveBeenCalled();
  });
  it("waits for the desktop receipt and prevents duplicate clicks", async () => {
    let complete!: (value: unknown) => void;
    mocks.restart.mockImplementation(
      () =>
        new Promise((resolve) => {
          complete = resolve;
        }),
    );
    render(<CodexDesktopRestartButton />);
    await userEvent.click(screen.getByRole("button"));
    expect(screen.getByRole("button")).toBeDisabled();
    expect(mocks.success).not.toHaveBeenCalled();
    await userEvent.click(screen.getByRole("button"));
    expect(mocks.restart).toHaveBeenCalledTimes(1);
    complete({ action: "restarted", desktop_pid: 42 });
    await waitFor(() => expect(mocks.success).toHaveBeenCalledTimes(1));
    expect(screen.getByRole("button")).toBeEnabled();
  });
  it("reports failure without reporting success and permits retry", async () => {
    mocks.restart.mockRejectedValue(new Error("exit timed out"));
    render(<CodexDesktopRestartButton />);
    await userEvent.click(screen.getByRole("button"));
    await waitFor(() => expect(mocks.error).toHaveBeenCalledTimes(1));
    expect(mocks.success).not.toHaveBeenCalled();
    expect(mocks.error.mock.calls[0][0]).toContain("exit timed out");
    expect(screen.getByRole("button")).toBeEnabled();
  });
});
