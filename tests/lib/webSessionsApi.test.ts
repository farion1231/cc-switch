import { afterEach, describe, expect, it, vi } from "vitest";
import { sessionsApi } from "@/lib/api/web/sessions";

const jsonResponse = (data: unknown) =>
  new Response(JSON.stringify({ success: true, data }), {
    status: 200,
    headers: { "Content-Type": "application/json" },
  });

describe("web sessions API", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("lists sessions from the scan endpoint", async () => {
    const sample = [
      {
        providerId: "claude",
        sessionId: "sid-1",
        title: "Hello",
        sourcePath: "/home/u/.claude/projects/p/sid-1.jsonl",
      },
    ];
    const fetchMock = vi
      .spyOn(globalThis, "fetch")
      .mockResolvedValue(jsonResponse(sample));

    const sessions = await sessionsApi.list();

    expect(fetchMock).toHaveBeenCalledWith(
      "/api/v1/sessions",
      expect.objectContaining({ method: "GET" }),
    );
    expect(sessions).toEqual(sample);
  });

  it("requests messages with providerId + sourcePath query params", async () => {
    const fetchMock = vi.spyOn(globalThis, "fetch").mockResolvedValue(
      jsonResponse([{ role: "user", content: "hi" }]),
    );

    await sessionsApi.getMessages(
      "claude",
      "/home/u/.claude/projects/p/sid-1.jsonl",
    );

    expect(fetchMock).toHaveBeenCalledWith(
      "/api/v1/sessions/messages?providerId=claude&sourcePath=%2Fhome%2Fu%2F.claude%2Fprojects%2Fp%2Fsid-1.jsonl",
      expect.objectContaining({ method: "GET" }),
    );
  });

  it("deletes a session by posting the full identity (provider, id, sourcePath)", async () => {
    const fetchMock = vi
      .spyOn(globalThis, "fetch")
      .mockResolvedValue(jsonResponse(true));

    const ok = await sessionsApi.delete({
      providerId: "claude",
      sessionId: "sid-1",
      sourcePath: "/home/u/.claude/projects/p/sid-1.jsonl",
    });

    expect(ok).toBe(true);
    expect(fetchMock).toHaveBeenCalledWith(
      "/api/v1/sessions/delete",
      expect.objectContaining({
        method: "POST",
        body: JSON.stringify({
          providerId: "claude",
          sessionId: "sid-1",
          sourcePath: "/home/u/.claude/projects/p/sid-1.jsonl",
        }),
      }),
    );
  });

  it("aggregates deleteMany results and reports per-item failures", async () => {
    let call = 0;
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      const url = String(input);
      if (url.endsWith("/sessions/delete")) {
        call += 1;
        // First call: success envelope. Second call: server returns a
        // failure envelope, which parseApiEnvelope surfaces as a thrown error.
        if (call === 1) return jsonResponse(true);
        return new Response(
          JSON.stringify({ success: false, error: "not found" }),
          { status: 200, headers: { "Content-Type": "application/json" } },
        );
      }
      throw new Error(`Unexpected request: ${url}`);
    });

    const results = await sessionsApi.deleteMany([
      { providerId: "claude", sessionId: "a", sourcePath: "/a" },
      { providerId: "codex", sessionId: "b", sourcePath: "/b" },
    ]);

    expect(results).toHaveLength(2);
    expect(results[0].success).toBe(true);
    expect(results[0].error).toBeUndefined();
    expect(results[1].success).toBe(false);
    expect(results[1].error).toBe("not found");
  });

  it("launchTerminal is a no-op in web mode", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    await expect(
      sessionsApi.launchTerminal({ command: "claude --resume x" }),
    ).resolves.toBe(false);
    expect(warn).toHaveBeenCalled();
  });
});
