import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { FullScreenPanel } from "@/components/common/FullScreenPanel";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";

/**
 * 浮层的 z-index 必须高于 FullScreenPanel（z-[60]），否则浮层内容被面板盖住。
 * Select / Popover 已定级为 z-[100]（见 commit f349d85e），DropdownMenu 需同级别。
 * 这里断言渲染出来的浮层自带该层级，而不是靠调用方传 className 兜底。
 */
describe("DropdownMenu z-index", () => {
  it("renders its content above FullScreenPanel's layer", async () => {
    const user = userEvent.setup();

    render(
      <FullScreenPanel isOpen title="Panel" onClose={() => undefined}>
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button>open</Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="start">
            <DropdownMenuItem>sonnet</DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </FullScreenPanel>,
    );

    await user.click(screen.getByRole("button", { name: "open" }));

    const item = await screen.findByRole("menuitem", { name: "sonnet" });
    // Portal 挂在 body 上，浮层层级由 DropdownMenuContent 自带。
    expect(item.closest("[role='menu']")).toHaveClass("z-[100]");
  });
});
