import { StrictMode } from "react";
import { render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { EnterTransition } from "@/components/common/EnterTransition";
import { directionByOrder } from "@/hooks/useEnterTransition";

// jsdom 没有 Web Animations：给 element.animate 一个假的，只记调用
const animate = vi.fn();
const originalAnimate = HTMLElement.prototype.animate;

function fakeAnimation() {
  return { pause: vi.fn(), play: vi.fn(), cancel: vi.fn() };
}

beforeEach(() => {
  animate.mockReset();
  animate.mockImplementation(fakeAnimation);
  HTMLElement.prototype.animate =
    animate as unknown as typeof HTMLElement.prototype.animate;
});

afterEach(() => {
  HTMLElement.prototype.animate = originalAnimate;
});

const firstTransform = (call: number) =>
  (animate.mock.calls[call][0] as Keyframe[])[0].transform;

describe("useEnterTransition", () => {
  it("does not play on mount under StrictMode when skipInitial is set", () => {
    // StrictMode 挂载时把 effect 跑两遍：第二遍 key 没变，不能当成一次切换
    render(
      <StrictMode>
        <EnterTransition transitionKey="a" from="start" />
      </StrictMode>,
    );
    expect(animate).not.toHaveBeenCalled();
  });

  it("plays once on mount under StrictMode when skipInitial is off", () => {
    render(
      <StrictMode>
        <EnterTransition transitionKey="a" from="above" skipInitial={false} />
      </StrictMode>,
    );
    expect(animate).toHaveBeenCalledTimes(1);
    expect(firstTransform(0)).toBe("translateY(-6px)");
  });

  it("plays once per key change with the computed direction", () => {
    const order = ["a", "b", "c"];
    const from = directionByOrder(order, "x");
    const { rerender } = render(
      <StrictMode>
        <EnterTransition transitionKey="a" from={from} />
      </StrictMode>,
    );
    rerender(
      <StrictMode>
        <EnterTransition transitionKey="c" from={from} />
      </StrictMode>,
    );
    rerender(
      <StrictMode>
        <EnterTransition transitionKey="b" from={from} />
      </StrictMode>,
    );
    expect(animate).toHaveBeenCalledTimes(2);
    expect(firstTransform(0)).toBe("translateX(12px)");
    expect(firstTransform(1)).toBe("translateX(-12px)");
  });
});

describe("useEnterTransition preserveHeader", () => {
  it("keeps the header bar and window controls still and moves the rest", () => {
    const page = (title: string) => (
      <EnterTransition
        transitionKey={title}
        from="above"
        preserveHeader
        data-testid="page"
      >
        <header data-page-header="" data-testid="bar">
          <div data-testid="title">{title}</div>
          <div data-testid="actions" />
          <div data-window-controls="" data-testid="controls" />
        </header>
        {/* display: contents 的一层没有盒子：动它的子元素 */}
        <div style={{ display: "contents" }} data-testid="scope">
          <div data-testid="body" />
        </div>
      </EnterTransition>
    );
    const { rerender, getByTestId } = render(page("A"));
    rerender(page("B"));

    const moved = animate.mock.contexts as HTMLElement[];
    const ids = moved.map((element) => element.dataset.testid).sort();
    expect(ids).toEqual(["actions", "body", "title"]);
    expect(moved).not.toContain(getByTestId("bar"));
    expect(moved).not.toContain(getByTestId("controls"));
    expect(moved).not.toContain(getByTestId("page"));
  });
});

describe("directionByOrder", () => {
  it("moves vertical content the same way as the selection", () => {
    const from = directionByOrder(["mcp", "skills"], "y");
    // 往下切：选中块往下走，内容从上面落下
    expect(from("mcp", "skills")).toBe("above");
    expect(from("skills", "mcp")).toBe("below");
  });
});
