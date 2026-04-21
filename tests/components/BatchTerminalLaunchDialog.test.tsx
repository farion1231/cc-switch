import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import {
  BatchTerminalLaunchDialog,
  type BatchTerminalLaunchOptions,
  type BatchTerminalLaunchTask,
} from "@/components/providers/BatchTerminalLaunchDialog";
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

const providers: Record<string, Provider> = {
  "claude-1": {
    id: "claude-1",
    name: "Claude Default",
    settingsConfig: {},
  },
  "claude-2": {
    id: "claude-2",
    name: "Claude Backup",
    settingsConfig: {},
  },
};

const renderDialog = (
  overrides: Partial<{
    app: "claude" | "codex";
    providers: Record<string, Provider>;
    onConfirm: (
      options: BatchTerminalLaunchOptions,
      tasks: BatchTerminalLaunchTask[],
    ) => void;
    onPickDirectory: () => Promise<string | null>;
  }> = {},
) => {
  const onConfirm = overrides.onConfirm ?? vi.fn();
  const onPickDirectory =
    overrides.onPickDirectory ?? vi.fn(async () => "/picked/path");

  render(
    <BatchTerminalLaunchDialog
      isOpen={true}
      app={overrides.app ?? "claude"}
      providers={overrides.providers ?? providers}
      onConfirm={onConfirm}
      onPickDirectory={onPickDirectory}
      onCancel={vi.fn()}
    />,
  );

  return { onConfirm, onPickDirectory };
};

describe("BatchTerminalLaunchDialog", () => {
  it("syncs directory slots with task count", () => {
    renderDialog();

    expect(screen.getAllByText(/Pane/)).toHaveLength(1);

    fireEvent.change(screen.getByLabelText("启动数量"), {
      target: { value: "3" },
    });

    expect(screen.getAllByText(/Pane/)).toHaveLength(3);

    fireEvent.change(screen.getByLabelText("启动数量"), {
      target: { value: "2" },
    });

    expect(screen.getAllByText(/Pane/)).toHaveLength(2);
  });

  it("submits tasks with global options after filling missing directories", async () => {
    const { onConfirm, onPickDirectory } = renderDialog({
      onPickDirectory: vi
        .fn()
        .mockResolvedValueOnce("/repo/one")
        .mockResolvedValueOnce("/repo/two"),
    });

    fireEvent.click(screen.getByLabelText("越权启动"));
    fireEvent.click(screen.getByLabelText("TG 通信"));
    fireEvent.change(screen.getByLabelText("启动数量"), {
      target: { value: "2" },
    });

    fireEvent.click(screen.getByRole("button", { name: "开始批量启动" }));

    expect(await screen.findByText("Claude Default")).toBeInTheDocument();
    expect(onPickDirectory).toHaveBeenCalledTimes(2);
    expect(onConfirm).toHaveBeenCalledWith(
      {
        bypass: true,
        enableTelegramChannel: true,
      },
      [
        {
          providerId: "claude-1",
          directories: ["/repo/one", "/repo/two"],
        },
      ],
    );
  });

  it("keeps the dialog open and does not submit when directory picking is cancelled", async () => {
    const { onConfirm } = renderDialog({
      onPickDirectory: vi.fn().mockResolvedValueOnce(null),
    });

    fireEvent.click(screen.getByRole("button", { name: "开始批量启动" }));

    expect(
      await screen.findByText("已取消目录选择，批量启动未执行。"),
    ).toBeInTheDocument();
    expect(onConfirm).not.toHaveBeenCalled();
  });

  it("can add and reorder tasks", () => {
    renderDialog();

    fireEvent.click(screen.getByRole("button", { name: "新增任务" }));
    const rows = screen.getAllByTestId("batch-terminal-task");

    fireEvent.change(within(rows[1]).getByLabelText("Provider"), {
      target: { value: "claude-2" },
    });
    fireEvent.click(within(rows[1]).getByRole("button", { name: "上移" }));

    const reorderedRows = screen.getAllByTestId("batch-terminal-task");
    expect(
      within(reorderedRows[0]).getByLabelText("Provider"),
    ).toHaveValue("claude-2");
  });
});
