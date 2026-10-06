import { beforeEach, describe, expect, it, vi } from "vitest";

const sonner = vi.hoisted(() => {
  let next = 0;
  const make = () =>
    vi.fn(
      (_message: unknown, options?: { id?: number }) => options?.id ?? ++next,
    );
  return { success: make(), error: make(), info: make(), warning: make() };
});

vi.mock("sonner", () => ({ toast: Object.assign(vi.fn(), sonner) }));

import { toast } from "@/lib/toast";

describe("toast", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    for (const fn of Object.values(sonner)) fn.mockClear();
  });

  it("refreshes the visible toast instead of stacking the same message", () => {
    const first = toast.success("所有技能已是最新版本");
    toast.success("所有技能已是最新版本");

    expect(sonner.success).toHaveBeenCalledTimes(2);
    expect(sonner.success.mock.calls[0]).toEqual(["所有技能已是最新版本"]);
    expect(sonner.success.mock.calls[1][1]).toEqual({ id: first });
  });

  it("shows a new toast once the previous one has gone", () => {
    toast.info("已复制");
    vi.advanceTimersByTime(2500);
    toast.info("已复制");

    expect(sonner.info.mock.calls[1]).toEqual(["已复制"]);
  });

  it("keeps different messages and kinds separate", () => {
    toast.error("保存失败");
    toast.success("保存失败");
    toast.error("另一个错误");

    expect(sonner.error.mock.calls[1]).toEqual(["另一个错误"]);
    expect(sonner.success.mock.calls[0]).toEqual(["保存失败"]);
  });
});
