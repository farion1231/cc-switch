import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { CodexDesktopRestartButton } from "@/components/providers/CodexDesktopRestartButton";

const mocks = vi.hoisted(() => ({
  restart: vi.fn(),
  success: vi.fn(),
  error: vi.fn(),
}));
vi.mock("@/lib/api/codexDesktop", () => ({
  restartCodexDesktop: mocks.restart,
}));
vi.mock("@/lib/toast", () => ({
  toast: { success: mocks.success, error: mocks.error },
}));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, values?: { error?: string }) =>
      key + (values?.error ?? ""),
  }),
}));

describe("CodexDesktopRestartButton", () => {
  beforeEach(() => vi.clearAllMocks());
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
