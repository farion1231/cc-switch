import { renderHook } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { toast } from "sonner";
import { useStackModelsChangedHint } from "@/hooks/useStackModelsChangedHint";
import type { ProxyStack } from "@/types/proxy";

vi.mock("sonner", () => ({
  toast: {
    info: vi.fn(),
  },
}));

function stackOf(...modelIds: string[][]): ProxyStack {
  return {
    active: true,
    members: modelIds.map((ids, index) => ({
      providerId: `p${index}`,
      modelIds: ids,
      route: index === 0,
    })),
  };
}

type Props = { appId: string; stack?: ProxyStack; updatedAt: number };

function renderHint(initial: Props) {
  return renderHook(
    ({ appId, stack, updatedAt }: Props) =>
      useStackModelsChangedHint(appId, stack, updatedAt),
    { initialProps: initial },
  );
}

describe("useStackModelsChangedHint", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("reminds to restart Claude Code when the published models change", () => {
    const { rerender } = renderHint({
      appId: "claude",
      stack: stackOf(["a"], ["b"]),
      updatedAt: 1,
    });
    // 第一次拿到名单只是记下来。
    expect(toast.info).not.toHaveBeenCalled();

    // 重查结果没变（比如窗口重新获得焦点）：不提示；换顺序也不算变。
    rerender({ appId: "claude", stack: stackOf(["b"], ["a"]), updatedAt: 2 });
    expect(toast.info).not.toHaveBeenCalled();

    // 编辑了名单里一家的模型。
    rerender({
      appId: "claude",
      stack: stackOf(["a"], ["b", "c"]),
      updatedAt: 3,
    });
    expect(toast.info).toHaveBeenCalledTimes(1);
    expect(toast.info).toHaveBeenCalledWith(
      "provider.stackModelsChanged",
      expect.anything(),
    );
  });

  it("skips the refetch right after adding or removing a member", () => {
    const { result, rerender } = renderHint({
      appId: "claude",
      stack: stackOf(["a"]),
      updatedAt: 1,
    });
    result.current();
    rerender({ appId: "claude", stack: stackOf(["a"], ["b"]), updatedAt: 2 });
    expect(toast.info).not.toHaveBeenCalled();

    // 跳过只管紧接着的一次：之后的变化照常提示。
    rerender({ appId: "claude", stack: stackOf(["a"]), updatedAt: 3 });
    expect(toast.info).toHaveBeenCalledTimes(1);
  });

  it("does not keep the skip past a refetch that changed nothing", () => {
    const { result, rerender } = renderHint({
      appId: "claude",
      stack: stackOf(["a"]),
      updatedAt: 1,
    });
    // 加入失败，重查结果没变：跳过在这一次用掉。
    result.current();
    rerender({ appId: "claude", stack: stackOf(["a"]), updatedAt: 2 });
    rerender({ appId: "claude", stack: stackOf(["a"], ["b"]), updatedAt: 3 });
    expect(toast.info).toHaveBeenCalledTimes(1);
  });

  it("stays quiet for Codex and outside Stack mode", () => {
    const { rerender } = renderHint({
      appId: "codex",
      stack: stackOf(["a"]),
      updatedAt: 1,
    });
    rerender({ appId: "codex", stack: stackOf(["a"], ["b"]), updatedAt: 2 });
    expect(toast.info).not.toHaveBeenCalled();

    // 退出 Stack 模式再进来：重新记，不拿进来之前的名单比。
    const claude = renderHint({
      appId: "claude",
      stack: stackOf(["a"]),
      updatedAt: 1,
    });
    claude.rerender({ appId: "claude", stack: undefined, updatedAt: 2 });
    claude.rerender({
      appId: "claude",
      stack: stackOf(["a"], ["b"]),
      updatedAt: 3,
    });
    expect(toast.info).not.toHaveBeenCalled();
  });
});
