import { act, fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet";

function renderSheet(props: { dismissOnOutsideClick?: boolean } = {}) {
  const onOpenChange = vi.fn();
  render(
    <>
      <button type="button">outside</button>
      <Sheet open onOpenChange={onOpenChange}>
        <SheetContent closeLabel="close" {...props}>
          <SheetHeader>
            <SheetTitle>Drawer</SheetTitle>
            <SheetDescription>body</SheetDescription>
          </SheetHeader>
          <input aria-label="draft" />
        </SheetContent>
      </Sheet>
    </>,
  );
  return { onOpenChange };
}

/** Radix 在下一帧才开始听外部 pointerdown：先让它挂上，再点遮罩外面 */
async function clickOutside() {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
  await act(async () => {
    const target = screen.getByRole("button", {
      name: "outside",
      hidden: true,
    });
    fireEvent.pointerDown(target, { button: 0, pointerType: "mouse" });
    fireEvent.click(target);
  });
}

describe("SheetContent", () => {
  it("ignores clicks on the overlay by default so drafts aren't lost", async () => {
    const { onOpenChange } = renderSheet();
    await clickOutside();
    expect(onOpenChange).not.toHaveBeenCalled();
    expect(screen.getByRole("dialog")).toBeInTheDocument();
  });

  it("closes on an outside click only when the drawer opts in", async () => {
    const { onOpenChange } = renderSheet({ dismissOnOutsideClick: true });
    await clickOutside();
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });
});
