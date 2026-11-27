import { beforeEach, describe, expect, it, vi } from "vitest";
import type {
  ContentRef,
  ImageRef,
  SessionContentSearchResult,
  TranscriptChunk,
} from "@/types";

const core = vi.hoisted(() => {
  class Channel<T> {
    onmessage: (message: T) => void = () => {};
  }
  return { invoke: vi.fn(), Channel };
});

describe("sessionsApi content search cancellation", () => {
  const emptyResult: SessionContentSearchResult = {
    hits: [],
    status: { running: false, processed: 0, total: 0 },
  };

  beforeEach(() => {
    core.invoke.mockReset();
  });

  it("does not send an already aborted search", async () => {
    const controller = new AbortController();
    controller.abort();
    await expect(
      sessionsApi.searchContent("的", undefined, controller.signal),
    ).rejects.toMatchObject({ name: "AbortError" });
    expect(core.invoke).not.toHaveBeenCalled();
  });

  it("cancels only the matching request and rejects a late successful reply", async () => {
    const pending: ((value: SessionContentSearchResult) => void)[] = [];
    core.invoke.mockImplementation((command) =>
      command === "search_session_content"
        ? new Promise((resolve) => pending.push(resolve))
        : Promise.resolve(),
    );
    const first = new AbortController();
    const second = new AbortController();
    const firstResult = sessionsApi.searchContent(
      "的",
      ["codex"],
      first.signal,
    );
    const secondResult = sessionsApi.searchContent(
      "的",
      ["codex"],
      second.signal,
    );
    const firstId = core.invoke.mock.calls[0][1].requestId;
    const secondId = core.invoke.mock.calls[1][1].requestId;
    expect(firstId).toEqual(expect.any(String));
    expect(firstId).not.toBe(secondId);
    first.abort();
    first.abort();
    expect(core.invoke.mock.calls.slice(2)).toEqual([
      ["cancel_session_content_search", { requestId: firstId }],
    ]);
    const cancelled = expect(firstResult).rejects.toMatchObject({
      name: "AbortError",
    });
    pending[0](emptyResult);
    pending[1](emptyResult);
    await cancelled;
    await expect(secondResult).resolves.toEqual(emptyResult);
    second.abort();
    expect(core.invoke).toHaveBeenCalledTimes(3);
  });

  it("preserves real SQL errors even if the signal was cancelled", async () => {
    let fail!: (error: unknown) => void;
    core.invoke.mockImplementation((command) =>
      command === "search_session_content"
        ? new Promise((_resolve, reject) => {
            fail = reject;
          })
        : Promise.resolve(),
    );
    const controller = new AbortController();
    const result = sessionsApi.searchContent(
      "的",
      undefined,
      controller.signal,
    );
    controller.abort();
    fail("Session index error: database disk image is malformed");
    await expect(result).rejects.toBe(
      "Session index error: database disk image is malformed",
    );
  });

  it.each([true, false])(
    "removes the abort listener after settling (success=%s)",
    async (success) => {
      const controller = new AbortController();
      if (success) core.invoke.mockResolvedValue(emptyResult);
      else core.invoke.mockRejectedValue("index open failed");
      const result = sessionsApi.searchContent(
        "的",
        undefined,
        controller.signal,
      );
      if (success) await expect(result).resolves.toEqual(emptyResult);
      else await expect(result).rejects.toBe("index open failed");
      controller.abort();
      expect(core.invoke).toHaveBeenCalledTimes(1);
    },
  );
});

vi.mock("@tauri-apps/api/core", () => core);

import { sessionsApi } from "@/lib/api/sessions";

describe("sessionsApi · 阅读页命令（P2 契约）", () => {
  beforeEach(() => {
    core.invoke.mockReset();
  });

  it("streamMessages：传 onChunk Channel，chunk 回调给调用方", async () => {
    const chunks: TranscriptChunk[] = [];
    core.invoke.mockImplementation(
      async (
        _command: string,
        args: { onChunk: InstanceType<typeof core.Channel> },
      ) => {
        args.onChunk.onmessage({ type: "done", payloadBytes: 3 });
      },
    );
    await sessionsApi.streamMessages("claude", "/a.jsonl", (chunk) =>
      chunks.push(chunk),
    );
    expect(core.invoke).toHaveBeenCalledWith("stream_session_messages", {
      providerId: "claude",
      sourcePath: "/a.jsonl",
      onChunk: expect.any(core.Channel),
    });
    expect(chunks).toEqual([{ type: "done", payloadBytes: 3 }]);
  });

  it("getBlockContent 用 contentRef 参数名，分页参数可省", async () => {
    const ref: ContentRef = { kind: "sidecar", relPath: "tool-results/x.txt" };
    core.invoke.mockResolvedValue({ text: "x", totalLen: 1, truncated: false });
    await sessionsApi.getBlockContent("claude", "/a.jsonl", ref);
    await sessionsApi.getBlockContent("claude", "/a.jsonl", ref, {
      offset: 10,
      limit: 20,
    });
    expect(core.invoke.mock.calls).toEqual([
      [
        "get_session_block_content",
        {
          providerId: "claude",
          sourcePath: "/a.jsonl",
          contentRef: ref,
          offset: undefined,
          limit: undefined,
        },
      ],
      [
        "get_session_block_content",
        {
          providerId: "claude",
          sourcePath: "/a.jsonl",
          contentRef: ref,
          offset: 10,
          limit: 20,
        },
      ],
    ]);
  });

  it("getImage", async () => {
    const image: ImageRef = {
      source: { kind: "local_file", path: "/tmp/a.png" },
      mediaType: "image/png",
      size: 3,
    };
    const bytes = new ArrayBuffer(3);
    core.invoke.mockResolvedValueOnce(bytes);
    await expect(
      sessionsApi.getImage("codex", "/b.jsonl", image),
    ).resolves.toBe(bytes);
    expect(core.invoke.mock.calls).toEqual([
      [
        "get_session_image",
        { providerId: "codex", sourcePath: "/b.jsonl", image },
      ],
    ]);
  });
});
