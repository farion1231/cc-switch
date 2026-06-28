import { afterEach, describe, expect, it, vi } from "vitest";

// Mock the Tauri invoke layer BEFORE importing the desktop sessions API so the
// module under test binds to the mock. This locks the desktop IPC contract
// (command names + argument shapes) the Rust `session_manager` commands expect.
const invokeMock = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

import { sessionsApi } from "@/lib/api/sessions";

describe("desktop sessions API (Tauri contract)", () => {
  afterEach(() => {
    invokeMock.mockReset();
  });

  it("list -> invoke('list_sessions')", async () => {
    invokeMock.mockResolvedValue([]);
    await sessionsApi.list();
    expect(invokeMock).toHaveBeenCalledWith("list_sessions");
  });

  it("getMessages -> invoke('get_session_messages', { providerId, sourcePath })", async () => {
    invokeMock.mockResolvedValue([]);
    await sessionsApi.getMessages("claude", "/path/to/session.jsonl");
    expect(invokeMock).toHaveBeenCalledWith("get_session_messages", {
      providerId: "claude",
      sourcePath: "/path/to/session.jsonl",
    });
  });

  it("delete -> invoke('delete_session', { providerId, sessionId, sourcePath })", async () => {
    invokeMock.mockResolvedValue(true);
    const ok = await sessionsApi.delete({
      providerId: "codex",
      sessionId: "sid-9",
      sourcePath: "/path/rollout.jsonl",
    });
    expect(ok).toBe(true);
    expect(invokeMock).toHaveBeenCalledWith("delete_session", {
      providerId: "codex",
      sessionId: "sid-9",
      sourcePath: "/path/rollout.jsonl",
    });
  });

  it("deleteMany -> invoke('delete_sessions', { items })", async () => {
    const items = [
      { providerId: "claude", sessionId: "a", sourcePath: "/a" },
      { providerId: "gemini", sessionId: "b", sourcePath: "/b" },
    ];
    invokeMock.mockResolvedValue(
      items.map((i) => ({ ...i, success: true })),
    );
    const results = await sessionsApi.deleteMany(items);
    expect(invokeMock).toHaveBeenCalledWith("delete_sessions", { items });
    expect(results).toHaveLength(2);
  });

  it("launchTerminal -> invoke('launch_session_terminal', {...})", async () => {
    invokeMock.mockResolvedValue(true);
    await sessionsApi.launchTerminal({
      command: "claude --resume sid",
      cwd: "/work",
      customConfig: null,
    });
    expect(invokeMock).toHaveBeenCalledWith("launch_session_terminal", {
      command: "claude --resume sid",
      cwd: "/work",
      customConfig: null,
    });
  });
});
