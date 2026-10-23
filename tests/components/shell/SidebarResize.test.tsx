import { useRef } from "react";
import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useSidebarResize } from "@/hooks/useSidebarResize";
import { sidebarWidth, useSidebarCollapsed } from "@/hooks/useSidebarCollapsed";

function Harness() {
  const ref = useRef<HTMLElement>(null);
  const { collapsed, toggle, setCollapsed, restoreCollapsedPreference } =
    useSidebarCollapsed(ref);
  const { width, resizeHandle } = useSidebarResize(
    ref,
    collapsed,
    setCollapsed,
    restoreCollapsedPreference,
  );
  return (
    <>
      <nav
        ref={ref}
        style={{ width: sidebarWidth(collapsed, width) }}
        data-testid="nav"
      >
        <button onClick={toggle}>Toggle</button>
        <div role="separator" {...resizeHandle} />
      </nav>
      <main id="content-area" />
      <output>{collapsed ? "collapsed" : "expanded"}</output>
    </>
  );
}
const nav = () => screen.getByTestId("nav");
const handle = () => screen.getByRole("separator");
const down = (x = 200) =>
  fireEvent.pointerDown(handle(), { button: 0, pointerId: 1, clientX: x });
const move = (x: number) =>
  fireEvent.pointerMove(handle(), { pointerId: 1, clientX: x });
const up = () => fireEvent.pointerUp(handle(), { pointerId: 1 });

beforeEach(() => {
  localStorage.clear();
  localStorage.setItem("cc-switch-sidebar-collapsed", "false");
  vi.stubGlobal("PointerEvent", MouseEvent);
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(
    function (this: HTMLElement) {
      return {
        width: parseFloat((this as HTMLElement).style.width) || 200,
      } as DOMRect;
    },
  );
  HTMLElement.prototype.setPointerCapture = vi.fn();
  HTMLElement.prototype.hasPointerCapture = vi.fn(() => false);
  vi.spyOn(window, "requestAnimationFrame").mockImplementation(() => 1);
  vi.spyOn(window, "cancelAnimationFrame").mockImplementation(() => {});
});
afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("sidebar resize", () => {
  it("flushes the final width when the last frame is cancelled and saved state is unchanged", () => {
    render(<Harness />);
    down();
    move(280);
    const callback = vi
      .mocked(window.requestAnimationFrame)
      .mock.calls.at(-1)![0];
    act(() => callback(16));
    expect(nav()).toHaveStyle({ width: "280px" });
    move(200);
    up();
    expect(nav()).toHaveStyle({ width: "200px" });
    expect(localStorage.getItem("cc-switch-sidebar-width")).toBe("200");
  });

  it("saves a resize and follows the pointer when dragged back out", () => {
    render(<Harness />);
    down();
    move(280);
    up();
    expect(nav()).toHaveStyle({ width: "280px" });
    expect(localStorage.getItem("cc-switch-sidebar-width")).toBe("280");
    down(280);
    move(120);
    expect(screen.getByText("collapsed")).toBeInTheDocument();
    expect(document.documentElement.style.cursor).toBe("col-resize");
    up();
    const rail = parseFloat(nav().style.width);
    down(rail);
    move(220);
    expect(screen.getByText("expanded")).toBeInTheDocument();
    expect(nav()).toHaveStyle({ width: "220px" });
    up();
  });
  it("keeps one drag alive across collapse, expansion and further resizing", () => {
    render(<Harness />);
    down();
    move(120);
    expect(screen.getByText("collapsed")).toBeInTheDocument();
    expect(nav()).toHaveAttribute("data-resizing", "true");
    move(160);
    expect(screen.getByText("expanded")).toBeInTheDocument();
    move(190);
    up();
    expect(nav()).toHaveStyle({ width: "190px" });
    expect(localStorage.getItem("cc-switch-sidebar-width")).toBe("190");
    expect(document.documentElement.style.cursor).toBe("");
  });
  it.each(["ltr", "rtl"])(
    "keeps the original grab offset through diagonal round trips (%s)",
    (direction) => {
      render(<Harness />);
      nav().style.direction = direction;
      const sign = direction === "rtl" ? -1 : 1;
      const start = 403;
      down(start);
      for (let pass = 0; pass < 5; pass++) {
        for (const requested of [120, 140, 140, 160, 500, 230]) {
          fireEvent.pointerMove(handle(), {
            pointerId: 1,
            clientX: start + (requested - 200) * sign,
            clientY: pass * 75 + requested,
          });
          if (requested === 140)
            expect(screen.getByText("collapsed")).toBeInTheDocument();
        }
      }
      up();
      expect(nav()).toHaveStyle({ width: "230px" });
      down(start + 30 * sign);
      move(start);
      up();
      expect(nav()).toHaveStyle({ width: "200px" });
    },
  );
  it("animates both snaps and yields to further pointer movement", () => {
    vi.stubGlobal(
      "DOMMatrixReadOnly",
      class {
        m41 = 0;
      },
    );
    const cancel = vi.fn();
    const animate = vi.fn(() => ({ cancel, onfinish: null, oncancel: null }));
    vi.stubGlobal("matchMedia", () => ({ matches: false }));
    Object.defineProperty(HTMLElement.prototype, "animate", {
      configurable: true,
      value: animate,
    });
    try {
      render(<Harness />);
      down();
      move(120);
      expect(animate).toHaveBeenCalledTimes(1);
      expect(animate.mock.calls[0]).toEqual([
        [
          { transform: "none" },
          { transform: expect.stringMatching(/^translateX\(-/) },
        ],
        expect.objectContaining({ duration: 200 }),
      ]);
      move(230);
      expect(cancel).toHaveBeenCalledTimes(1);
      expect(nav()).toHaveStyle({ width: "230px" });
      expect(animate).toHaveBeenCalledTimes(2);
      expect(animate.mock.calls[1]).toEqual([
        [
          { transform: "none" },
          { transform: expect.stringMatching(/^translateX\([0-9]/) },
        ],
        expect.objectContaining({ duration: 200 }),
      ]);
      move(240);
      expect(cancel).toHaveBeenCalledTimes(2);
      move(120);
      fireEvent.keyDown(window, { key: "Escape" });
      expect(cancel).toHaveBeenCalledTimes(3);
      expect(nav()).toHaveStyle({ width: "200px" });
      expect(nav().style.zIndex).toBe("");
      expect(document.querySelector('[aria-hidden="true"]')).toBeNull();
    } finally {
      delete (HTMLElement.prototype as Partial<HTMLElement>).animate;
    }
  });
  it("skips the snap animation with reduced motion", () => {
    const animate = vi.fn();
    Object.defineProperty(HTMLElement.prototype, "animate", {
      configurable: true,
      value: animate,
    });
    vi.stubGlobal("matchMedia", () => ({ matches: true }));
    try {
      render(<Harness />);
      down();
      move(120);
      expect(screen.getByText("collapsed")).toBeInTheDocument();
      expect(animate).not.toHaveBeenCalled();
      up();
    } finally {
      delete (HTMLElement.prototype as Partial<HTMLElement>).animate;
    }
  });
  it("cancels a snapped drag back to its original mode and width", () => {
    render(<Harness />);
    down();
    move(120);
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.getByText("expanded")).toBeInTheDocument();
    expect(nav()).toHaveStyle({ width: "200px" });
    expect(localStorage.getItem("cc-switch-sidebar-width")).toBeNull();
    fireEvent.click(screen.getByText("Toggle"));
    const rail = parseFloat(nav().style.width);
    down(rail);
    move(220);
    fireEvent.pointerCancel(handle());
    expect(screen.getByText("collapsed")).toBeInTheDocument();
    expect(nav()).toHaveStyle({ width: `${rail}px` });
  });
  it("restores automatic window behavior after cancelling a snap round trip", () => {
    localStorage.removeItem("cc-switch-sidebar-collapsed");
    vi.stubGlobal("innerWidth", 1200);
    render(<Harness />);
    down();
    move(120);
    move(160);
    fireEvent.keyDown(window, { key: "Escape" });
    expect(localStorage.getItem("cc-switch-sidebar-collapsed")).toBeNull();
    expect(screen.getByText("expanded")).toBeInTheDocument();
    vi.stubGlobal("innerWidth", 900);
    fireEvent(window, new Event("resize"));
    expect(screen.getByText("collapsed")).toBeInTheDocument();
    vi.stubGlobal("innerWidth", 1200);
    fireEvent(window, new Event("resize"));
    expect(screen.getByText("expanded")).toBeInTheDocument();
  });
  it("does not collapse from an ordinary small resize, and Escape cancels without saving", () => {
    render(<Harness />);
    down();
    move(160);
    expect(screen.getByText("expanded")).toBeInTheDocument();
    fireEvent.keyDown(window, { key: "Escape" });
    up();
    expect(nav()).toHaveStyle({ width: "200px" });
    expect(localStorage.getItem("cc-switch-sidebar-width")).toBeNull();
    expect(document.documentElement.style.userSelect).toBe("");
  });
  it("button collapse interrupts dragging and late pointer events cannot expand it", () => {
    render(<Harness />);
    down();
    move(300);
    fireEvent.click(screen.getByText("Toggle"));
    move(350);
    up();
    expect(screen.getByText("collapsed")).toBeInTheDocument();
    expect(parseFloat(nav().style.width)).toBeLessThan(100);
    expect(localStorage.getItem("cc-switch-sidebar-width")).toBeNull();
  });
  it("supports keyboard collapse, expansion, bounds and reset", () => {
    render(<Harness />);
    fireEvent.keyDown(handle(), { key: "End" });
    expect(nav()).toHaveStyle({ width: "360px" });
    fireEvent.keyDown(handle(), { key: "Home" });
    expect(screen.getByText("collapsed")).toBeInTheDocument();
    fireEvent.keyDown(handle(), { key: "Enter" });
    expect(nav()).toHaveStyle({ width: "360px" });
    fireEvent.doubleClick(handle());
    expect(nav()).toHaveStyle({ width: "200px" });
  });
  it("cleans up pointer cancellation and unmount", () => {
    const { unmount } = render(<Harness />);
    down();
    move(300);
    fireEvent.pointerCancel(handle());
    expect(nav()).toHaveStyle({ width: "200px" });
    down();
    unmount();
    expect(document.documentElement.style.cursor).toBe("");
    expect(document.documentElement.style.userSelect).toBe("");
  });
});
