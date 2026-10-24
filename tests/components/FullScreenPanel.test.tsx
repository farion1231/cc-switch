import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { FullScreenPanel } from "@/components/common/FullScreenPanel";
import { DRAG_REGION_ENABLED } from "@/lib/platform";

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

describe("FullScreenPanel motion", () => {
  it("is opaque from the first render, including after reopening", async () => {
    const panel = (isOpen: boolean) => (
      <FullScreenPanel isOpen={isOpen} title="Editor" onClose={() => undefined}>
        editor content
      </FullScreenPanel>
    );
    const view = render(panel(true));
    const surface = () => screen.getByText("editor content").closest(".bg-app");
    expect(surface()).toHaveStyle({ opacity: "1" });

    view.rerender(panel(false));
    // If AnimatePresence retains the closing page, it must remain opaque.
    const closing = screen.queryByText("editor content")?.closest(".bg-app");
    if (closing) expect(closing).toHaveStyle({ opacity: "1" });
    await waitFor(() =>
      expect(screen.queryByText("editor content")).toBeNull(),
    );

    view.rerender(panel(true));
    expect(surface()).toHaveStyle({ opacity: "1" });
  });

  it("keeps the explicit horizontal navigation transition", () => {
    render(
      <FullScreenPanel
        isOpen
        title="Authorization"
        onClose={() => undefined}
        motionPreset="slide-from-right"
      >
        authorization content
      </FullScreenPanel>,
    );
    const surface = screen
      .getByText("authorization content")
      .closest(".bg-app");
    expect(surface).toHaveStyle({ transform: "translateX(100%)" });
    expect(surface).not.toHaveStyle({ opacity: "0" });
  });
});

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

describe("FullScreenPanel Escape", () => {
  const Stacked = ({
    innerOpen,
    onOuterClose,
    onInnerClose,
  }: {
    innerOpen: boolean;
    onOuterClose: () => void;
    onInnerClose: () => void;
  }) => (
    <>
      <FullScreenPanel isOpen title="Pricing" onClose={onOuterClose}>
        pricing
      </FullScreenPanel>
      <FullScreenPanel
        isOpen={innerOpen}
        title="Edit pricing"
        onClose={onInnerClose}
      >
        <input aria-label="price" />
      </FullScreenPanel>
    </>
  );

  it("closes only the topmost of two stacked panels", async () => {
    const onOuterClose = vi.fn();
    const onInnerClose = vi.fn();
    const view = render(
      <Stacked
        innerOpen={false}
        onOuterClose={onOuterClose}
        onInnerClose={onInnerClose}
      />,
    );
    // 子页后打开，叠在上面
    view.rerender(
      <Stacked
        innerOpen
        onOuterClose={onOuterClose}
        onInnerClose={onInnerClose}
      />,
    );

    fireEvent.keyDown(document.body, { key: "Escape" });
    expect(onInnerClose).toHaveBeenCalledTimes(1);
    expect(onOuterClose).not.toHaveBeenCalled();

    // 输入框里的 ESC 留给输入框，哪层都不关
    fireEvent.keyDown(screen.getByLabelText("price"), { key: "Escape" });
    expect(onInnerClose).toHaveBeenCalledTimes(1);
    expect(onOuterClose).not.toHaveBeenCalled();

    // 子页关掉后，下一次 ESC 才轮到下面那层
    view.rerender(
      <Stacked
        innerOpen={false}
        onOuterClose={onOuterClose}
        onInnerClose={onInnerClose}
      />,
    );
    await waitFor(() => expect(screen.queryByLabelText("price")).toBeNull());
    fireEvent.keyDown(document.body, { key: "Escape" });
    expect(onOuterClose).toHaveBeenCalledTimes(1);
    expect(onInnerClose).toHaveBeenCalledTimes(1);
  });
});

describe("FullScreenPanel header", () => {
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
