import { act, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import WorkspaceFileEditor from "@/components/workspace/WorkspaceFileEditor";
import { workspaceApi } from "@/lib/api/workspace";

vi.mock("@/lib/api/workspace", () => ({
  workspaceApi: { readFile: vi.fn(), writeFile: vi.fn() },
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

describe("WorkspaceFileEditor opening", () => {
  it("waits for content and ignores an older file read", async () => {
    const first = deferred();
    const second = deferred();
    vi.mocked(workspaceApi.readFile)
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(second.promise);
    const onClose = vi.fn();
    const view = render(
      <WorkspaceFileEditor isOpen filename="A.md" onClose={onClose} />,
    );
    expect(screen.queryByRole("heading")).toBeNull();
    expect(screen.queryByText("prompts.loading")).toBeNull();
    view.rerender(
      <WorkspaceFileEditor isOpen filename="B.md" onClose={onClose} />,
    );
    await act(async () => second.resolve("B content"));
    expect(screen.getByRole("textbox")).toHaveValue("B content");
    expect(screen.getByRole("textbox").closest(".bg-app")).toHaveStyle({
      opacity: "1",
    });
    await act(async () => first.resolve("A content"));
    expect(screen.getByRole("textbox")).toHaveValue("B content");
  });

  it("does not reopen after closing during a read", async () => {
    const read = deferred();
    vi.mocked(workspaceApi.readFile).mockReturnValueOnce(read.promise);
    const onClose = vi.fn();
    const view = render(
      <WorkspaceFileEditor isOpen filename="A.md" onClose={onClose} />,
    );
    view.rerender(
      <WorkspaceFileEditor isOpen={false} filename="A.md" onClose={onClose} />,
    );
    await act(async () => read.resolve("late content"));
    expect(screen.queryByRole("textbox")).toBeNull();
  });

  it("returns to the list on failure instead of opening an empty editor", async () => {
    const read = deferred();
    vi.mocked(workspaceApi.readFile).mockReturnValueOnce(read.promise);
    const onClose = vi.fn();
    render(<WorkspaceFileEditor isOpen filename="A.md" onClose={onClose} />);
    await act(async () => read.reject(new Error("read failed")));
    expect(onClose).toHaveBeenCalledOnce();
    expect(screen.queryByRole("textbox")).toBeNull();
  });
});
