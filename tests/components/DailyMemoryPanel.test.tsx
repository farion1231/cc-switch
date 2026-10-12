import { act, fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import DailyMemoryPanel from "@/components/workspace/DailyMemoryPanel";
import { workspaceApi } from "@/lib/api/workspace";

vi.mock("@/lib/api/workspace", () => ({
  workspaceApi: { listDailyMemoryFiles: vi.fn(), readDailyMemoryFile: vi.fn() },
}));
vi.mock("@/components/MarkdownEditor", () => ({
  default: ({ value }: { value: string }) => (
    <textarea aria-label="Content" readOnly value={value} />
  ),
}));

function deferred() {
  let resolve!: (value: string) => void;
  let reject!: (reason: Error) => void;
  const promise = new Promise<string>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

beforeEach(() => {
  vi.mocked(workspaceApi.listDailyMemoryFiles).mockResolvedValue([
    {
      filename: "2026-10-10.md",
      date: "2026-10-10",
      sizeBytes: 10,
      modifiedAt: 0,
      preview: "First note",
    },
    {
      filename: "2026-10-11.md",
      date: "2026-10-11",
      sizeBytes: 10,
      modifiedAt: 0,
      preview: "Second note",
    },
  ]);
});

describe("DailyMemoryPanel opening", () => {
  it("keeps the list visible until the latest selected note is ready", async () => {
    const first = deferred();
    const second = deferred();
    vi.mocked(workspaceApi.readDailyMemoryFile)
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(second.promise);
    render(<DailyMemoryPanel isOpen onClose={vi.fn()} />);
    fireEvent.click(await screen.findByRole("button", { name: /First note/ }));
    expect(screen.getByText("Second note")).toBeVisible();
    expect(screen.queryByRole("textbox")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: /Second note/ }));
    await act(async () => second.resolve("Second content"));
    expect(screen.getByRole("textbox")).toHaveValue("Second content");
    await act(async () => first.resolve("First content"));
    expect(screen.getByRole("textbox")).toHaveValue("Second content");
  });

  it("ignores a read completed after closing and reopening", async () => {
    const read = deferred();
    vi.mocked(workspaceApi.readDailyMemoryFile).mockReturnValueOnce(
      read.promise,
    );
    const onClose = vi.fn();
    const view = render(<DailyMemoryPanel isOpen onClose={onClose} />);
    fireEvent.click(await screen.findByRole("button", { name: /First note/ }));
    view.rerender(<DailyMemoryPanel isOpen={false} onClose={onClose} />);
    await act(async () => read.resolve("Late content"));
    view.rerender(<DailyMemoryPanel isOpen onClose={onClose} />);
    expect(await screen.findByText("First note")).toBeVisible();
    expect(screen.queryByRole("textbox")).toBeNull();
  });

  it("keeps the list visible when reading fails", async () => {
    const read = deferred();
    vi.mocked(workspaceApi.readDailyMemoryFile).mockReturnValueOnce(
      read.promise,
    );
    render(<DailyMemoryPanel isOpen onClose={vi.fn()} />);
    fireEvent.click(await screen.findByRole("button", { name: /First note/ }));
    await act(async () => read.reject(new Error("read failed")));
    expect(screen.getByText("First note")).toBeVisible();
    expect(screen.queryByRole("textbox")).toBeNull();
  });
});
