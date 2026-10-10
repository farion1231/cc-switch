import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { FullScreenPanel } from "@/components/common/FullScreenPanel";
import { WindowControlsContext } from "@/components/shell/AppPageHeader";
import { DRAG_REGION_ENABLED } from "@/lib/platform";

// 模拟 Linux 默认禁用拖动区域，避免依赖测试运行机器的平台。
vi.mock("@/lib/platform", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/platform")>()),
  DRAG_REGION_ENABLED: false,
  DRAG_REGION_ATTR: {},
  DRAG_REGION_STYLE: {},
}));

const Panels = ({ innerOpen }: { innerOpen: boolean }) => (
  <>
    <FullScreenPanel isOpen title="Outer" onClose={() => undefined}>
      outer
    </FullScreenPanel>
    <FullScreenPanel isOpen={innerOpen} title="Inner" onClose={() => undefined}>
      inner
    </FullScreenPanel>
  </>
);

describe("FullScreenPanel body scroll locking", () => {
  afterEach(() => {
    document.body.style.overflow = "";
  });

  it("keeps the body locked when a nested panel closes", () => {
    document.body.style.overflow = "clip";
    const view = render(<Panels innerOpen />);

    expect(document.body.style.overflow).toBe("hidden");

    view.rerender(<Panels innerOpen={false} />);
    expect(document.body.style.overflow).toBe("hidden");

    view.unmount();
    expect(document.body.style.overflow).toBe("clip");
  });
});

describe("FullScreenPanel header", () => {
  it("enables dragging with app window controls and updates through the portal", () => {
    const onBack = vi.fn();
    const onMinimize = vi.fn();
    const panel = (enabled: boolean) => (
      <WindowControlsContext.Provider
        value={enabled ? <button onClick={onMinimize}>Minimize</button> : null}
      >
        <FullScreenPanel isOpen title="Edit MCP" onClose={onBack}>
          body
        </FullScreenPanel>
      </WindowControlsContext.Provider>
    );
    const view = render(panel(false));
    const title = screen.getByRole("heading", { name: "Edit MCP" });
    expect(view.container).not.toContainElement(title);
    const dragRegions = [title, title.parentElement!, title.closest("header")!];
    for (const enabled of [false, true, false]) {
      view.rerender(panel(enabled));
      for (const element of dragRegions) {
        expect(element.hasAttribute("data-tauri-drag-region")).toBe(enabled);
      }
      if (enabled) {
        const back = screen.getByRole("button", { name: "common.back" });
        const minimize = screen.getByRole("button", { name: "Minimize" });
        for (const button of [back, minimize]) {
          expect(button).not.toHaveAttribute("data-tauri-drag-region");
          fireEvent.click(button);
        }
      }
    }
    expect(onBack).toHaveBeenCalledOnce();
    expect(onMinimize).toHaveBeenCalledOnce();
  });

  it("lets a long title shrink and truncate instead of pushing the window controls away", () => {
    const title = "Edit " + "a-very-long-user-provided-name-".repeat(8);
    render(
      <FullScreenPanel isOpen title={title} onClose={() => undefined}>
        body
      </FullScreenPanel>,
    );
    const heading = screen.getByRole("heading", { name: title });
    expect(heading).toHaveClass("min-w-0", "truncate");
    expect(heading.parentElement).toHaveClass("flex-1");
    expect(heading.parentElement).not.toHaveClass("shrink-0");
  });

  it("keeps the whole title area as a window drag region", () => {
    render(
      <FullScreenPanel isOpen title="Edit MCP" onClose={() => undefined}>
        body
      </FullScreenPanel>,
    );
    const heading = screen.getByRole("heading", { name: "Edit MCP" });
    // Tauri 只认按下的元素自身带属性：标题和铺满页头的标题区都得带
    for (const element of [heading, heading.parentElement!]) {
      expect(element.hasAttribute("data-tauri-drag-region")).toBe(
        DRAG_REGION_ENABLED,
      );
    }
    expect(
      screen.getByRole("button", { name: "common.back" }),
    ).not.toHaveAttribute("data-tauri-drag-region");
  });
});
