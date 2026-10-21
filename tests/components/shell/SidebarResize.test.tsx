import { useRef } from "react";
import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useSidebarResize } from "@/hooks/useSidebarResize";
import { sidebarWidth, useSidebarCollapsed } from "@/hooks/useSidebarCollapsed";

function Harness() {
  const ref = useRef<HTMLElement>(null);
  const { collapsed, toggle, setCollapsed } = useSidebarCollapsed(ref);
  const { width, resizeHandle } = useSidebarResize(
    ref,
    collapsed,
    setCollapsed,
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
  it("saves a resize, snaps closed, and restores that width when dragged out", () => {
    render(<Harness />);
    down();
    move(280);
    up();
    expect(nav()).toHaveStyle({ width: "280px" });
    expect(localStorage.getItem("cc-switch-sidebar-width")).toBe("280");
    down(280);
    move(120);
    expect(screen.getByText("collapsed")).toBeInTheDocument();
    expect(document.documentElement.style.cursor).toBe("");
    const rail = parseFloat(nav().style.width);
    down(rail);
    move(rail + 30);
    expect(screen.getByText("expanded")).toBeInTheDocument();
    expect(nav()).toHaveStyle({ width: "280px" });
  });
  it("does not collapse from an ordinary small resize, and Escape cancels without saving", () => {
    render(<Harness />);
    down();
    move(150);
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
