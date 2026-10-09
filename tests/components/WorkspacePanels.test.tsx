import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import WorkspaceFilesPanel from "@/components/workspace/WorkspaceFilesPanel";
import DailyMemoryPanel from "@/components/workspace/DailyMemoryPanel";
import { workspaceApi } from "@/lib/api/workspace";

vi.mock("@/lib/api/workspace", () => ({
  workspaceApi: {
    getRootDirectory: vi.fn(),
    readFile: vi.fn(),
    listDailyMemoryFiles: vi.fn(),
    openDirectory: vi.fn(),
  },
}));

beforeEach(() => {
  vi.mocked(workspaceApi.getRootDirectory).mockResolvedValue(
    "/custom/workspace",
  );
  vi.mocked(workspaceApi.readFile).mockResolvedValue(null);
  vi.mocked(workspaceApi.listDailyMemoryFiles).mockResolvedValue([]);
  vi.mocked(workspaceApi.openDirectory).mockResolvedValue(undefined);
});

describe("OpenClaw workspace panels", () => {
  it("shows the resolved workspace root and opens that directory", async () => {
    render(<WorkspaceFilesPanel />);
    const path = await screen.findByText("/custom/workspace/");
    expect(
      screen.queryByText("~/.openclaw/workspace/"),
    ).not.toBeInTheDocument();
    fireEvent.click(path);
    expect(workspaceApi.openDirectory).toHaveBeenCalledWith("workspace");
  });

  it("shows the resolved memory root and refreshes it when reopened", async () => {
    const { rerender } = render(<DailyMemoryPanel isOpen onClose={() => {}} />);
    fireEvent.click(await screen.findByText("/custom/workspace/memory/"));
    expect(workspaceApi.openDirectory).toHaveBeenCalledWith("memory");
    rerender(<DailyMemoryPanel isOpen={false} onClose={() => {}} />);
    vi.mocked(workspaceApi.getRootDirectory).mockResolvedValue(
      "/new/workspace",
    );
    rerender(<DailyMemoryPanel isOpen onClose={() => {}} />);
    expect(
      await screen.findByText("/new/workspace/memory/"),
    ).toBeInTheDocument();
  });

  it("does not display a guessed default when root resolution fails", async () => {
    vi.mocked(workspaceApi.getRootDirectory).mockRejectedValue(
      new Error("invalid config"),
    );
    const log = vi.spyOn(console, "error").mockImplementation(() => {});
    render(<WorkspaceFilesPanel />);
    await waitFor(() => expect(log).toHaveBeenCalled());
    expect(
      screen.queryByText("~/.openclaw/workspace/"),
    ).not.toBeInTheDocument();
    expect(screen.queryByText("/custom/workspace/")).not.toBeInTheDocument();
    log.mockRestore();
  });
});
