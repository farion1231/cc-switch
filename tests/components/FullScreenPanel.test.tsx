import { render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
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
