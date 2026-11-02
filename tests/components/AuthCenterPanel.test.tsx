import { fireEvent, render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AuthCenterPanel } from "@/components/settings/AuthCenterPanel";

vi.mock("@/components/providers/forms/ClaudeOfficialSection", () => ({
  ClaudeOfficialSection: () => <div data-testid="claude" />,
}));
vi.mock("@/components/providers/forms/CopilotAuthSection", () => ({
  CopilotAuthSection: () => <div data-testid="copilot" />,
}));
vi.mock("@/components/providers/forms/CodexOAuthSection", () => ({
  CodexOAuthSection: () => <div data-testid="codex" />,
}));
vi.mock("@/components/providers/forms/XaiOAuthSection", () => ({
  XaiOAuthSection: () => <div data-testid="xai" />,
}));
vi.mock("@/components/providers/forms/AgyOAuthSection", () => ({
  AgyOAuthSection: () => <div data-testid="agy" />,
}));
vi.mock("@/components/providers/KimiAccountLogin", () => ({
  KimiAccountLogin: () => <div data-testid="kimi" />,
}));
vi.mock("@/components/providers/DshAccount", () => ({
  DshAccountLogin: () => <div data-testid="dsh" />,
}));

let resizeCallbacks: Array<() => void> = [];

const rect = (top: number) => ({ top }) as DOMRect;

/** 正文 overflow-y:auto，外层 #content-area overflow:hidden，和 FullScreenPanel 一致 */
const renderInPanel = () => {
  const outer = document.createElement("main");
  outer.style.overflow = "hidden";
  const container = document.createElement("div");
  container.style.overflowY = "auto";
  outer.appendChild(container);
  document.body.appendChild(outer);

  Object.defineProperty(container, "clientHeight", { value: 400 });
  Object.defineProperty(container, "scrollHeight", { value: 2000 });
  container.getBoundingClientRect = () => rect(100);

  const result = render(<AuthCenterPanel authScrollTarget="codex_oauth" />, {
    container,
  });
  return { ...result, outer, container };
};

/** 让 codex 卡片在文档里的位置是 offset，屏幕位置随正文滚动变化 */
const placeCodexAt = (container: HTMLElement, offset: () => number) => {
  const codex = container.querySelector('[data-testid="codex"]')!
    .parentElement as HTMLElement;
  codex.getBoundingClientRect = () =>
    rect(100 + offset() - container.scrollTop);
};

/** 卡片位置摆好后重新定位一次（首次挂载时 jsdom 还量不出位置） */
const retarget = (rerender: (ui: React.ReactElement) => void) => {
  rerender(<AuthCenterPanel authScrollTarget={null} />);
  resizeCallbacks = [];
  rerender(<AuthCenterPanel authScrollTarget="codex_oauth" />);
};

describe("AuthCenterPanel scroll target", () => {
  beforeEach(() => {
    resizeCallbacks = [];
    vi.stubGlobal(
      "ResizeObserver",
      class {
        constructor(callback: () => void) {
          resizeCallbacks.push(callback);
        }
        observe() {}
        disconnect() {}
      },
    );
    vi.spyOn(window, "requestAnimationFrame").mockImplementation((cb) => {
      cb(0);
      return 0;
    });
    Element.prototype.scrollIntoView = vi.fn();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
    document.body.innerHTML = "";
  });

  it("scrolls only the panel body, never the outer content area", () => {
    let offset = 600;
    const { outer, container, rerender } = renderInPanel();
    placeCodexAt(container, () => offset);
    retarget(rerender);

    expect(container.scrollTop).toBe(584);
    expect(outer.scrollTop).toBe(0);
    expect(outer.scrollLeft).toBe(0);
    expect(Element.prototype.scrollIntoView).not.toHaveBeenCalled();

    // 上面几组加载完把目标挤下去：跟着重新对齐
    offset = 800;
    resizeCallbacks.forEach((callback) => callback());
    expect(container.scrollTop).toBe(784);
  });

  it("stops re-aligning once the user scrolls", () => {
    let offset = 600;
    const { container, rerender } = renderInPanel();
    placeCodexAt(container, () => offset);
    retarget(rerender);
    expect(container.scrollTop).toBe(584);

    fireEvent.wheel(container);
    container.scrollTop = 300;
    offset = 800;
    resizeCallbacks.forEach((callback) => callback());
    expect(container.scrollTop).toBe(300);
  });

  it("disables scroll anchoring while pinning and restores it afterwards", () => {
    const { container, rerender, unmount } = renderInPanel();
    placeCodexAt(container, () => 600);
    retarget(rerender);
    // 浏览器滚动锚定会自己改 scrollTop，被误判成用户接管
    expect(container.style.overflowAnchor).toBe("none");

    fireEvent.wheel(container);
    expect(container.style.overflowAnchor).toBe("");

    retarget(rerender);
    expect(container.style.overflowAnchor).toBe("none");
    unmount();
    expect(container.style.overflowAnchor).toBe("");
  });
});
