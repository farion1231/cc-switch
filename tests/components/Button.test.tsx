import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { Button } from "@/components/ui/button";

describe("Button", () => {
  it("展开/折叠触发器按压时不缩放", () => {
    render(
      <Button aria-expanded="false" className="w-full">
        Advanced
      </Button>,
    );

    // 全宽触发器一旦缩放，左边缘的箭头会在 mousedown 后移出按钮，
    // mouseup 便落在父元素上，click 不会触发面板展开。
    expect(screen.getByRole("button")).toHaveClass(
      "[&[aria-expanded]]:active:scale-100",
    );
  });
});
