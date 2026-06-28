import { describe, it, expect, vi, beforeEach } from "vitest";
import { workspaceApi } from "./workspace";

const mocks = vi.hoisted(() => ({
  get: vi.fn(),
  put: vi.fn(),
  del: vi.fn(),
}));

vi.mock("../web-client", () => ({
  get: mocks.get,
  put: mocks.put,
  del: mocks.del,
}));

describe("web workspaceApi", () => {
  beforeEach(() => {
    vi.resetAllMocks();
  });

  it("reads a workspace file", async () => {
    mocks.get.mockResolvedValueOnce("# agents");
    const result = await workspaceApi.readFile("AGENTS.md");
    expect(result).toBe("# agents");
    expect(mocks.get).toHaveBeenCalledWith("/workspace/file/AGENTS.md");
  });

  it("writes a workspace file", async () => {
    mocks.put.mockResolvedValueOnce(undefined);
    await workspaceApi.writeFile("AGENTS.md", "# agents");
    expect(mocks.put).toHaveBeenCalledWith("/workspace/file/AGENTS.md", {
      content: "# agents",
    });
  });

  it("lists daily memory files", async () => {
    mocks.get.mockResolvedValueOnce([{ filename: "2026-06-18.md" }]);
    const result = await workspaceApi.listDailyMemoryFiles();
    expect(result).toHaveLength(1);
    expect(mocks.get).toHaveBeenCalledWith("/workspace/daily-memory");
  });

  it("reads a daily memory file", async () => {
    mocks.get.mockResolvedValueOnce("# notes");
    const result = await workspaceApi.readDailyMemoryFile("2026-06-18.md");
    expect(result).toBe("# notes");
    expect(mocks.get).toHaveBeenCalledWith(
      "/workspace/daily-memory/2026-06-18.md",
    );
  });

  it("writes a daily memory file", async () => {
    mocks.put.mockResolvedValueOnce(undefined);
    await workspaceApi.writeDailyMemoryFile("2026-06-18.md", "# notes");
    expect(mocks.put).toHaveBeenCalledWith(
      "/workspace/daily-memory/2026-06-18.md",
      { content: "# notes" },
    );
  });

  it("deletes a daily memory file", async () => {
    mocks.del.mockResolvedValueOnce(undefined);
    await workspaceApi.deleteDailyMemoryFile("2026-06-18.md");
    expect(mocks.del).toHaveBeenCalledWith(
      "/workspace/daily-memory/2026-06-18.md",
    );
  });

  it("searches daily memory files", async () => {
    mocks.get.mockResolvedValueOnce([{ filename: "2026-06-18.md" }]);
    const result = await workspaceApi.searchDailyMemoryFiles("hello");
    expect(result).toHaveLength(1);
    expect(mocks.get).toHaveBeenCalledWith(
      "/workspace/daily-memory/search?query=hello",
    );
  });

  it("opens a directory", async () => {
    mocks.get.mockResolvedValueOnce("/path/to/workspace");
    const result = await workspaceApi.openDirectory("workspace");
    expect(result).toBe("/path/to/workspace");
    expect(mocks.get).toHaveBeenCalledWith(
      "/workspace/directory?subdir=workspace",
    );
  });
});
