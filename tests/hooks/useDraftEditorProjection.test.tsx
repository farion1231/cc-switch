import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useDraftEditorProjection } from "@/components/providers/forms/hooks/useDraftEditorProjection";

const getEditorView = vi.fn();
const toastError = vi.fn();

vi.mock("@/lib/api", () => ({
  providersApi: {
    getEditorView: (...args: unknown[]) => getEditorView(...args),
  },
}));

vi.mock("@/lib/toast", () => ({
  toast: { error: (...args: unknown[]) => toastError(...args) },
}));

const snapshot = (fingerprint: string) => ({
  selector: "custom",
  legacyRoute: true,
  legacyFingerprint: fingerprint,
  profileReferenced: false,
});
type View = {
  settings: Record<string, unknown>;
  codexSnapshot?: ReturnType<typeof snapshot>;
};

const deferred = <T,>() => {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
};

describe("useDraftEditorProjection", () => {
  beforeEach(() => {
    getEditorView.mockReset();
    toastError.mockReset();
  });

  it("连续切换预设时只认最后一次投影", async () => {
    const first = deferred<View>();
    const second = deferred<View>();
    getEditorView
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(second.promise);
    const onBase = vi.fn();
    const apply = vi.fn();
    const { result } = renderHook(() =>
      useDraftEditorProjection("codex", onBase),
    );

    act(() => {
      result.current.projectDraft({ config: "a" }, undefined, apply);
      result.current.projectDraft({ config: "b" }, "official", apply);
    });
    expect(getEditorView).toHaveBeenLastCalledWith(
      "codex",
      { config: "b" },
      "official",
    );

    await act(async () => {
      second.resolve({
        settings: { config: "B" },
        codexSnapshot: snapshot("b"),
      });
      await second.promise;
    });
    await act(async () => {
      first.resolve({
        settings: { config: "A" },
        codexSnapshot: snapshot("a"),
      });
      await first.promise;
    });

    expect(apply).toHaveBeenCalledTimes(1);
    expect(apply).toHaveBeenCalledWith({ config: "B" });
    // 底和投影成它的草稿一起交出去，保存时后端按草稿分开预设带的字段。
    expect(onBase).toHaveBeenLastCalledWith(
      { config: "B" },
      { config: "b" },
      snapshot("b"),
    );
  });

  it("投影失败时提示并保持没有底", async () => {
    const failing = deferred<View>();
    getEditorView.mockReturnValueOnce(failing.promise);
    const onBase = vi.fn();
    const apply = vi.fn();
    const { result } = renderHook(() =>
      useDraftEditorProjection("gemini", onBase),
    );

    act(() => {
      result.current.projectDraft({ env: {} }, undefined, apply);
    });
    await act(async () => {
      failing.reject(new Error("broken settings.json"));
      await failing.promise.catch(() => undefined);
    });

    expect(apply).not.toHaveBeenCalled();
    expect(onBase).toHaveBeenCalledTimes(1);
    expect(onBase).toHaveBeenCalledWith(null);
    expect(toastError).toHaveBeenCalledTimes(1);
  });

  it("作废后迟到的投影不再生效", async () => {
    const pending = deferred<View>();
    getEditorView.mockReturnValueOnce(pending.promise);
    const onBase = vi.fn();
    const apply = vi.fn();
    const { result } = renderHook(() =>
      useDraftEditorProjection("grokbuild", onBase),
    );

    act(() => {
      result.current.projectDraft({ config: "x" }, undefined, apply);
      result.current.clearDraftProjection();
    });
    await act(async () => {
      pending.resolve({ settings: { config: "X" } });
      await pending.promise;
    });

    expect(apply).not.toHaveBeenCalled();
    expect(onBase).toHaveBeenLastCalledWith(null);
  });

  it("没有接收方时不发请求（编辑对话框等场景）", () => {
    const { result } = renderHook(() => useDraftEditorProjection("codex"));
    act(() => {
      result.current.projectDraft({ config: "x" }, undefined, vi.fn());
    });
    expect(getEditorView).not.toHaveBeenCalled();
  });

  it("新请求失败时清除旧快照，迟到的失败不影响新投影", async () => {
    const old = deferred<View>();
    const current = deferred<View>();
    getEditorView
      .mockReturnValueOnce(old.promise)
      .mockReturnValueOnce(current.promise);
    const onBase = vi.fn();
    const apply = vi.fn();
    const { result } = renderHook(() =>
      useDraftEditorProjection("codex", onBase),
    );
    act(() => {
      result.current.projectDraft({ config: "old" }, undefined, apply);
      result.current.projectDraft({ config: "current" }, undefined, apply);
    });
    await act(async () => {
      current.resolve({
        settings: { config: "shown" },
        codexSnapshot: snapshot("current"),
      });
      await current.promise;
      old.reject(new Error("stale failure"));
      await old.promise.catch(() => undefined);
    });
    expect(toastError).not.toHaveBeenCalled();
    expect(onBase).toHaveBeenLastCalledWith(
      { config: "shown" },
      { config: "current" },
      snapshot("current"),
    );
    getEditorView.mockRejectedValueOnce(new Error("current failure"));
    await act(async () => {
      result.current.projectDraft({ config: "next" }, undefined, apply);
    });
    expect(onBase).toHaveBeenLastCalledWith(null);
    expect(toastError).toHaveBeenCalledTimes(1);
    expect(apply).toHaveBeenCalledTimes(1);
  });

  it("卸载后不再接收显示或快照", async () => {
    const pending = deferred<View>();
    getEditorView.mockReturnValueOnce(pending.promise);
    const onBase = vi.fn();
    const apply = vi.fn();
    const { result, unmount } = renderHook(() =>
      useDraftEditorProjection("codex", onBase),
    );
    act(() => result.current.projectDraft({ config: "x" }, undefined, apply));
    unmount();
    await act(async () => {
      pending.resolve({
        settings: { config: "X" },
        codexSnapshot: snapshot("x"),
      });
      await pending.promise;
    });
    expect(apply).not.toHaveBeenCalled();
    expect(onBase).toHaveBeenCalledTimes(1);
    expect(onBase).toHaveBeenLastCalledWith(null);
  });
});
