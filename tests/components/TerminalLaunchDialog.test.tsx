import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { TerminalLaunchDialog } from "@/components/providers/TerminalLaunchDialog";
import type { Provider } from "@/types";

vi.mock("@/components/ui/dialog", () => ({
  Dialog: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
  DialogContent: ({ children }: { children: React.ReactNode }) => (
    <div>{children}</div>
  ),
  DialogHeader: ({ children }: { children: React.ReactNode }) => (
    <div>{children}</div>
  ),
  DialogTitle: ({ children }: { children: React.ReactNode }) => (
    <h1>{children}</h1>
  ),
  DialogDescription: ({ children }: { children: React.ReactNode }) => (
    <div>{children}</div>
  ),
  DialogFooter: ({ children }: { children: React.ReactNode }) => (
    <div>{children}</div>
  ),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, opts?: string | Record<string, unknown>) => {
      if (typeof opts === "string") return opts;
      return (opts?.defaultValue as string | undefined) ?? key;
    },
  }),
}));

const provider: Provider = {
  id: "claude-1",
  name: "Claude Default",
  settingsConfig: {},
};

describe("TerminalLaunchDialog", () => {
  it("勾选 TG 通信后会将状态传给确认回调", () => {
    const handleConfirm = vi.fn();

    render(
      <TerminalLaunchDialog
        isOpen={true}
        provider={provider}
        app="claude"
        onConfirm={handleConfirm}
        onCancel={vi.fn()}
      />,
    );

    fireEvent.click(screen.getByRole("checkbox"));
    fireEvent.click(screen.getByRole("button", { name: "普通启动" }));

    expect(handleConfirm).toHaveBeenCalledWith(false, true);
  });

  it("重新打开弹窗后会重置 TG 通信勾选状态", () => {
    const handleConfirm = vi.fn();

    const { rerender } = render(
      <TerminalLaunchDialog
        isOpen={true}
        provider={provider}
        app="claude"
        onConfirm={handleConfirm}
        onCancel={vi.fn()}
      />,
    );

    fireEvent.click(screen.getByRole("checkbox"));

    rerender(
      <TerminalLaunchDialog
        isOpen={false}
        provider={provider}
        app="claude"
        onConfirm={handleConfirm}
        onCancel={vi.fn()}
      />,
    );

    rerender(
      <TerminalLaunchDialog
        isOpen={true}
        provider={provider}
        app="claude"
        onConfirm={handleConfirm}
        onCancel={vi.fn()}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "普通启动" }));

    expect(handleConfirm).toHaveBeenCalledWith(false, false);
  });
});
