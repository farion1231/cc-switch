import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { render } from "@testing-library/react";
import {
  requestReveal,
  resetRevealForTests,
  useRevealRequest,
} from "@/lib/searchReveal";

let frames: Map<number, FrameRequestCallback>;
let now: number;

const advance = (ms: number) => {
  // 每帧 50ms，直到推进完 ms
  for (let elapsed = 0; elapsed < ms; elapsed += 50) {
    now += 50;
    const callbacks = [...frames.values()];
    frames.clear();
    callbacks.forEach((callback) => callback(now));
  }
};

/** jsdom 没有布局：给挂上去的锚点一个「看得见」的矩形，并接住闪烁动画 */
function mountAnchor(target: string) {
  const element = document.createElement("div");
  element.setAttribute("data-search-anchor", target);
  element.getClientRects = () =>
    [new DOMRect(0, 0, 10, 10)] as unknown as DOMRectList;
  const animate = vi.fn().mockReturnValue({ cancel: vi.fn() });
  element.animate = animate;
  document.body.appendChild(element);
  return { element, animate };
}

beforeEach(() => {
  frames = new Map();
  now = 0;
  let frameId = 0;
  vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
    frames.set(++frameId, callback);
    return frameId;
  });
  vi.stubGlobal("cancelAnimationFrame", (id: number) => frames.delete(id));
  vi.spyOn(performance, "now").mockImplementation(() => now);
});

afterEach(() => {
  resetRevealForTests();
  document.body.innerHTML = "";
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("requestReveal", () => {
  it("waits for the target to mount, then flashes it", () => {
    requestReveal("mcp:fetch");
    advance(200);
    const { animate } = mountAnchor("mcp:fetch");
    expect(animate).not.toHaveBeenCalled();

    advance(50);
    expect(animate).toHaveBeenCalledTimes(1);
    expect(animate).toHaveBeenCalledWith(
      expect.any(Array),
      expect.objectContaining({ iterations: 2 }),
    );
  });

  it("ignores a hidden copy of the target", () => {
    const hidden = mountAnchor("skill:a");
    hidden.element.getClientRects = () => [] as unknown as DOMRectList;
    requestReveal("skill:a");
    advance(100);
    expect(hidden.animate).not.toHaveBeenCalled();
  });

  it("falls back to another anchor when the target never renders", () => {
    const { animate } = mountAnchor("setting:launch");
    requestReveal("setting:silent", { fallback: "setting:launch" });
    advance(300);
    expect(animate).not.toHaveBeenCalled();
    advance(200);
    expect(animate).toHaveBeenCalledTimes(1);
  });

  it("gives up after a few seconds", () => {
    requestReveal("provider:missing");
    advance(4100);
    expect(frames.size).toBe(0);
    const { animate } = mountAnchor("provider:missing");
    advance(100);
    expect(animate).not.toHaveBeenCalled();
  });

  it("cancels the previous request when a new one starts", () => {
    requestReveal("mcp:old");
    requestReveal("mcp:new");
    const old = mountAnchor("mcp:old");
    const next = mountAnchor("mcp:new");
    advance(100);
    expect(old.animate).not.toHaveBeenCalled();
    expect(next.animate).toHaveBeenCalledTimes(1);
  });
});

describe("useRevealRequest", () => {
  function Listener({ onReveal }: { onReveal: (id: string) => void }) {
    useRevealRequest("skill:", onReveal);
    return null;
  }

  it("hands matching requests to the page with the prefix removed", () => {
    const onReveal = vi.fn();
    render(<Listener onReveal={onReveal} />);
    requestReveal("mcp:fetch");
    requestReveal("skill:pdf");
    expect(onReveal).toHaveBeenCalledTimes(1);
    expect(onReveal).toHaveBeenCalledWith("pdf");
  });

  it("delivers a pending request to a page that mounts afterwards", () => {
    requestReveal("skill:pdf");
    const onReveal = vi.fn();
    render(<Listener onReveal={onReveal} />);
    expect(onReveal).toHaveBeenCalledWith("pdf");
  });

  it("does not replay a request that already finished", () => {
    mountAnchor("skill:pdf");
    requestReveal("skill:pdf");
    advance(50);
    const onReveal = vi.fn();
    render(<Listener onReveal={onReveal} />);
    expect(onReveal).not.toHaveBeenCalled();
  });
});
