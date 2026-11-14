import type { ReactNode } from "react";
import { act, renderHook, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type {
  ContentRef,
  ImageRef,
  SessionMessage,
  TranscriptChunk,
} from "@/types";

const api = vi.hoisted(() => ({
  getMessages: vi.fn(),
  streamMessages: vi.fn(),
  getBlockContent: vi.fn(),
  getImage: vi.fn(),
}));

vi.mock("@/lib/api/sessions", () => ({ sessionsApi: api }));

import {
  applyTranscriptChunk,
  BLOCK_PAGE_CHARS,
  clearSessionImageCache,
  EMPTY_TRANSCRIPT,
  loadTranscript,
  SESSION_IMAGE_CACHE_SIZE,
  sessionKeys,
  stableKey,
  TRANSCRIPT_DONE_GRACE_MS,
  useBlockContent,
  useSessionImage,
  useSessionMessagesQuery,
  useSessionTranscript,
} from "@/lib/query/sessions";
import { useSessionMessagesQuery as legacyExport } from "@/lib/query";

const message = (content: string, turnId = "t1"): SessionMessage => ({
  role: "user",
  content,
  turnId,
  blocks: [{ type: "text", text: content }],
});

const header: TranscriptChunk = {
  type: "header",
  total: 2,
  turns: [],
  cached: true,
  parseMs: 1,
};

/** 让 streamMessages 依次推送 chunk，再按 outcome 结束 */
const streamOf =
  (chunks: TranscriptChunk[], outcome?: Error) =>
  async (
    _providerId: string,
    _sourcePath: string,
    onChunk: (chunk: TranscriptChunk) => void,
  ) => {
    chunks.forEach((chunk) => onChunk(chunk));
    if (outcome) throw outcome;
  };

const wrapperWith = (client: QueryClient) =>
  function Wrapper({ children }: { children: ReactNode }) {
    return (
      <QueryClientProvider client={client}>{children}</QueryClientProvider>
    );
  };

const newClient = () =>
  new QueryClient({ defaultOptions: { queries: { retry: false } } });

beforeEach(() => {
  Object.values(api).forEach((fn) => fn.mockReset());
});

describe("stableKey / sessionKeys", () => {
  it("键顺序无关，跳过 undefined", () => {
    expect(stableKey({ b: 1, a: [{ y: "2", x: undefined }] })).toBe(
      stableKey({ a: [{ y: "2" }], b: 1 }),
    );
    expect(stableKey({ b: 1, a: 2 })).toBe('{"a":2,"b":1}');
    expect(sessionKeys.blockContent("p", "s")).toEqual([
      "sessionBlockContent",
      "p",
      "s",
      null,
    ]);
  });
});

describe("applyTranscriptChunk", () => {
  it("header / messages（按 start 拼接、可覆盖）/ done / error", () => {
    let state = applyTranscriptChunk(EMPTY_TRANSCRIPT, header);
    expect(state.header).toBe(header);
    state = applyTranscriptChunk(state, {
      type: "messages",
      start: 0,
      messages: [message("a")],
    });
    state = applyTranscriptChunk(state, {
      type: "messages",
      start: 1,
      messages: [message("b")],
    });
    expect(state.messages.map((m) => m.content)).toEqual(["a", "b"]);
    state = applyTranscriptChunk(state, {
      type: "messages",
      start: 1,
      messages: [message("c")],
    });
    expect(state.messages.map((m) => m.content)).toEqual(["a", "c"]);
    state = applyTranscriptChunk(state, { type: "done", payloadBytes: 9 });
    expect(state).toMatchObject({ done: true, payloadBytes: 9 });
    expect(() =>
      applyTranscriptChunk(state, { type: "error", message: "boom" }),
    ).toThrow("boom");
    expect(EMPTY_TRANSCRIPT.messages).toEqual([]);
  });
});

describe("loadTranscript", () => {
  it("逐块回调进度，Done 后返回", async () => {
    api.streamMessages.mockImplementation(
      streamOf([
        header,
        { type: "messages", start: 0, messages: [message("a")] },
        { type: "done", payloadBytes: 1 },
      ]),
    );
    const progress = vi.fn();
    const state = await loadTranscript("claude", "/a", progress);
    expect(progress).toHaveBeenCalledTimes(3);
    expect(state).toMatchObject({ done: true, payloadBytes: 1 });
    expect(state.messages).toHaveLength(1);
  });

  it("命令返回后迟迟没有 Done：宽限期后按已收到的内容结束", async () => {
    vi.useFakeTimers();
    try {
      api.streamMessages.mockImplementation(streamOf([header]));
      const pending = loadTranscript("claude", "/a");
      await vi.advanceTimersByTimeAsync(TRANSCRIPT_DONE_GRACE_MS);
      await expect(pending).resolves.toMatchObject({ header, done: true });
    } finally {
      vi.useRealTimers();
    }
  });

  it("后端先推 Error 再 reject：抛出该错误，不回退", async () => {
    api.streamMessages.mockImplementation(
      streamOf(
        [{ type: "error", message: "会话已更新" }],
        new Error("会话已更新"),
      ),
    );
    await expect(loadTranscript("claude", "/a")).rejects.toThrow("会话已更新");
    expect(api.getMessages).not.toHaveBeenCalled();
  });

  it("只推了 Error 但命令正常返回：仍然抛错", async () => {
    api.streamMessages.mockImplementation(
      streamOf([{ type: "error", message: "bad" }]),
    );
    await expect(loadTranscript("claude", "/a")).rejects.toThrow("bad");
  });

  it("一个 chunk 都没收到就失败：回退到 get_session_messages 并自算轮次索引", async () => {
    api.streamMessages.mockRejectedValue(new Error("unknown command"));
    api.getMessages.mockResolvedValue([message("hello", "t1")]);
    const state = await loadTranscript("claude", "/a");
    expect(state).toMatchObject({
      done: true,
      header: { type: "header", total: 1, cached: false, parseMs: 0 },
    });
    expect(state.header?.turns.map((turn) => turn.turnId)).toEqual(["t1"]);
  });
});

describe("useSessionTranscript", () => {
  it("首包到达即有数据，Done 后结束流式状态", async () => {
    let push: (chunk: TranscriptChunk) => void = () => {};
    let finish: () => void = () => {};
    api.streamMessages.mockImplementation(
      (_p: string, _s: string, onChunk: (chunk: TranscriptChunk) => void) =>
        new Promise<void>((resolve) => {
          push = onChunk;
          finish = resolve;
        }),
    );
    const client = newClient();
    const { result } = renderHook(() => useSessionTranscript("claude", "/a"), {
      wrapper: wrapperWith(client),
    });
    await waitFor(() => expect(api.streamMessages).toHaveBeenCalled());
    expect(result.current).toMatchObject({
      header: null,
      messages: [],
      progress: { loaded: 0, total: null },
    });

    act(() => {
      push(header);
      push({ type: "messages", start: 0, messages: [message("a")] });
    });
    await waitFor(() => expect(result.current.messages).toHaveLength(1));
    expect(result.current).toMatchObject({
      isStreaming: true,
      progress: { loaded: 1, total: 2 },
    });

    act(() => {
      push({ type: "done", payloadBytes: 1 });
      finish();
    });
    await waitFor(() => expect(result.current.isStreaming).toBe(false));
    await waitFor(() => expect(client.isFetching()).toBe(0));
    expect(result.current.isError).toBe(false);

    // 后台刷新不推中间态
    api.streamMessages.mockImplementation(
      streamOf([
        header,
        { type: "messages", start: 0, messages: [message("x"), message("y")] },
        { type: "done", payloadBytes: 2 },
      ]),
    );
    const setSpy = vi.spyOn(client, "setQueryData");
    await act(async () => {
      await result.current.refetch();
    });
    await waitFor(() =>
      expect(result.current.messages.map((m) => m.content)).toEqual(["x", "y"]),
    );
    expect(setSpy).not.toHaveBeenCalled();
  });

  it("缺参数时不发请求", () => {
    const { result } = renderHook(() => useSessionTranscript(undefined, "/a"), {
      wrapper: wrapperWith(newClient()),
    });
    expect(api.streamMessages).not.toHaveBeenCalled();
    expect(result.current.isLoading).toBe(false);
  });
});

describe("useSessionMessagesQuery", () => {
  it("一次性读取；旧入口 @/lib/query 仍可用", async () => {
    expect(legacyExport).toBe(useSessionMessagesQuery);
    api.getMessages.mockResolvedValue([message("a")]);
    const { result } = renderHook(
      () => useSessionMessagesQuery("claude", "/a"),
      { wrapper: wrapperWith(newClient()) },
    );
    await waitFor(() => expect(result.current.data).toHaveLength(1));
    expect(api.getMessages).toHaveBeenCalledWith("claude", "/a");
  });
});

describe("useBlockContent", () => {
  const ref: ContentRef = { kind: "sidecar", relPath: "tool-results/a.txt" };

  it("分页加载更多，拼接全文", async () => {
    api.getBlockContent
      .mockResolvedValueOnce({
        text: "abc",
        totalLen: 6,
        truncated: true,
        nextOffset: 3,
      })
      .mockResolvedValueOnce({ text: "def", totalLen: 6, truncated: false });
    const { result } = renderHook(() => useBlockContent("claude", "/a", ref), {
      wrapper: wrapperWith(newClient()),
    });
    await waitFor(() => expect(result.current.text).toBe("abc"));
    expect(result.current).toMatchObject({
      totalLen: 6,
      hasMore: true,
      tooLarge: false,
    });
    expect(api.getBlockContent).toHaveBeenCalledWith("claude", "/a", ref, {
      offset: 0,
      limit: BLOCK_PAGE_CHARS,
    });
    act(() => result.current.loadMore());
    await waitFor(() => expect(result.current.text).toBe("abcdef"));
    expect(result.current.hasMore).toBe(false);
    expect(api.getBlockContent).toHaveBeenLastCalledWith("claude", "/a", ref, {
      offset: 3,
      limit: BLOCK_PAGE_CHARS,
    });
  });

  it("累计到 2MB 停止并标记 tooLarge；没给 nextOffset 也停", async () => {
    const big = "x".repeat(2 * 1024 * 1024);
    api.getBlockContent.mockResolvedValueOnce({
      text: big,
      totalLen: big.length + 10,
      truncated: true,
      nextOffset: big.length,
    });
    const { result } = renderHook(() => useBlockContent("claude", "/a", ref), {
      wrapper: wrapperWith(newClient()),
    });
    await waitFor(() => expect(result.current.tooLarge).toBe(true));
    expect(result.current.hasMore).toBe(false);

    api.getBlockContent.mockResolvedValueOnce({
      text: "a",
      totalLen: 5,
      truncated: true,
    });
    const other = renderHook(
      () =>
        useBlockContent("claude", "/a", { kind: "sidecar", relPath: "other" }),
      { wrapper: wrapperWith(newClient()) },
    );
    await waitFor(() => expect(other.result.current.text).toBe("a"));
    expect(other.result.current.hasMore).toBe(false);
  });

  it("没有 ref 或 enabled=false 不请求", () => {
    const disabled = renderHook(
      () => useBlockContent("claude", "/a", ref, { enabled: false }),
      { wrapper: wrapperWith(newClient()) },
    );
    const missing = renderHook(
      () => useBlockContent("claude", "/a", undefined),
      { wrapper: wrapperWith(newClient()) },
    );
    expect(api.getBlockContent).not.toHaveBeenCalled();
    expect(disabled.result.current).toMatchObject({
      text: "",
      totalLen: null,
      hasMore: false,
      tooLarge: false,
    });
    expect(missing.result.current.isLoading).toBe(false);
  });
});

describe("useSessionImage", () => {
  const image = (path: string): ImageRef => ({
    source: { kind: "local_file", path },
    mediaType: "image/png",
    size: 3,
  });
  let created = 0;
  const createObjectURL = vi.fn(() => `blob:${++created}`);
  const revokeObjectURL = vi.fn();

  beforeEach(() => {
    created = 0;
    createObjectURL.mockClear();
    revokeObjectURL.mockClear();
    Object.assign(URL, { createObjectURL, revokeObjectURL });
    api.getImage.mockResolvedValue(new ArrayBuffer(3));
  });

  afterEach(() => {
    clearSessionImageCache();
  });

  it("取字节转 Blob URL；同一张图共享缓存；失败给 error", async () => {
    const first = renderHook(() =>
      useSessionImage("codex", "/a", image("/1.png")),
    );
    expect(first.result.current.isLoading).toBe(true);
    await waitFor(() => expect(first.result.current.url).toBe("blob:1"));
    const second = renderHook(() =>
      useSessionImage("codex", "/a", image("/1.png")),
    );
    expect(second.result.current.url).toBe("blob:1");
    expect(api.getImage).toHaveBeenCalledTimes(1);
    expect(createObjectURL).toHaveBeenCalledWith(expect.any(Blob));

    api.getImage.mockRejectedValueOnce(new Error("svg rejected"));
    const failed = renderHook(() =>
      useSessionImage("codex", "/a", image("/bad.svg")),
    );
    await waitFor(() => expect(failed.result.current.error).toBeTruthy());
    expect(failed.result.current.url).toBeNull();
  });

  it("未启用不请求；卸载后留在 LRU，超出上限才 revoke 没人用的", async () => {
    const idle = renderHook(() =>
      useSessionImage("codex", "/a", image("/x.png"), { enabled: false }),
    );
    expect(idle.result.current).toEqual({
      url: null,
      isLoading: false,
      error: null,
    });
    renderHook(() => useSessionImage("codex", "/a", undefined));
    expect(api.getImage).not.toHaveBeenCalled();

    const held = renderHook(() =>
      useSessionImage("codex", "/a", image("/held.png")),
    );
    await waitFor(() => expect(held.result.current.url).toBeTruthy());
    for (let i = 0; i < SESSION_IMAGE_CACHE_SIZE; i += 1) {
      const hook = renderHook(() =>
        useSessionImage("codex", "/a", image(`/${i}.png`)),
      );
      await waitFor(() => expect(hook.result.current.url).toBeTruthy());
      hook.unmount();
    }
    // 第 33 张进来时淘汰最久未用且没人在用的（/0.png），正在用的 held 保留
    expect(revokeObjectURL).toHaveBeenCalledTimes(1);
    expect(revokeObjectURL).toHaveBeenCalledWith("blob:2");
    held.unmount();
  });

  it("上限内的图全在用时，新加载的那张也拿得到 URL，不会被立刻淘汰", async () => {
    const mounted = [];
    for (let i = 0; i < SESSION_IMAGE_CACHE_SIZE; i += 1) {
      const hook = renderHook(() =>
        useSessionImage("codex", "/a", image(`/busy-${i}.png`)),
      );
      await waitFor(() => expect(hook.result.current.url).toBeTruthy());
      mounted.push(hook);
    }
    const extra = renderHook(() =>
      useSessionImage("codex", "/a", image("/extra.png")),
    );
    await waitFor(() => expect(extra.result.current.isLoading).toBe(false));
    expect(extra.result.current.url).toBe(
      `blob:${SESSION_IMAGE_CACHE_SIZE + 1}`,
    );
    expect(revokeObjectURL).not.toHaveBeenCalled();
    extra.unmount();
    mounted.forEach((hook) => hook.unmount());
  });

  it("上限内的图全在用时，两张同时加载完也都拿得到 URL，不会互相淘汰", async () => {
    const mounted = [];
    for (let i = 0; i < SESSION_IMAGE_CACHE_SIZE; i += 1) {
      const hook = renderHook(() =>
        useSessionImage("codex", "/a", image(`/busy-${i}.png`)),
      );
      await waitFor(() => expect(hook.result.current.url).toBeTruthy());
      mounted.push(hook);
    }
    const resolvers: Array<(value: ArrayBuffer) => void> = [];
    api.getImage.mockImplementation(
      () => new Promise<ArrayBuffer>((ok) => resolvers.push(ok)),
    );
    const x = renderHook(() => useSessionImage("codex", "/a", image("/x.png")));
    const y = renderHook(() => useSessionImage("codex", "/a", image("/y.png")));
    await act(async () => {
      resolvers.forEach((resolve) => resolve(new ArrayBuffer(3)));
    });
    await waitFor(() => expect(y.result.current.isLoading).toBe(false));
    expect(x.result.current.url).toBeTruthy();
    expect(y.result.current.url).toBeTruthy();
    expect(revokeObjectURL).not.toHaveBeenCalled();
    [x, y, ...mounted].forEach((hook) => hook.unmount());
  });

  it("加载中卸载：不再更新状态", async () => {
    let resolve: (value: ArrayBuffer) => void = () => {};
    let reject: (error: Error) => void = () => {};
    api.getImage
      .mockImplementationOnce(
        () => new Promise<ArrayBuffer>((ok) => (resolve = ok)),
      )
      .mockImplementationOnce(
        () => new Promise<ArrayBuffer>((_, fail) => (reject = fail)),
      );
    const ok = renderHook(() =>
      useSessionImage("codex", "/a", image("/s.png")),
    );
    const bad = renderHook(() =>
      useSessionImage("codex", "/a", image("/f.png")),
    );
    ok.unmount();
    bad.unmount();
    await act(async () => {
      resolve(new ArrayBuffer(1));
      reject(new Error("late"));
    });
    expect(ok.result.current).toMatchObject({ url: null, isLoading: true });
    expect(bad.result.current.error).toBeNull();
  });
});
