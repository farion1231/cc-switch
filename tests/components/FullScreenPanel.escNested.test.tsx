import { fireEvent, render } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { FullScreenPanel } from "@/components/common/FullScreenPanel";

describe("FullScreenPanel nesting", () => {
  it("lets Esc close only the panel opened last", () => {
    const outer = vi.fn();
    const inner = vi.fn();
    const ui = (innerOpen: boolean) => (
      <FullScreenPanel isOpen title="outer" onClose={outer}>
        <FullScreenPanel isOpen={innerOpen} title="inner" onClose={inner}>
          x
        </FullScreenPanel>
      </FullScreenPanel>
    );
    const { rerender } = render(ui(false));
    rerender(ui(true));
    fireEvent.keyDown(window, { key: "Escape" });
    expect(inner).toHaveBeenCalledTimes(1);
    expect(outer).not.toHaveBeenCalled();
  });
});
