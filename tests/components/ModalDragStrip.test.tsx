import { act, fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { WindowControlsContext } from "@/components/shell/AppPageHeader";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetTitle,
} from "@/components/ui/sheet";
import { DRAG_REGION_ENABLED } from "@/lib/platform";

function renderDialog(onInteractOutside?: () => void) {
  return render(
    <WindowControlsContext.Provider value={<button>Minimize</button>}>
      <Dialog open>
        <DialogContent onInteractOutside={onInteractOutside}>
          <DialogTitle>Title</DialogTitle>
          <DialogDescription>Body</DialogDescription>
        </DialogContent>
      </Dialog>
    </WindowControlsContext.Provider>,
  );
}

function renderSheet() {
  const onOpenChange = vi.fn();
  render(
    <WindowControlsContext.Provider value={<button>Minimize</button>}>
      <Sheet open onOpenChange={onOpenChange}>
        <SheetContent closeLabel="close" dismissOnOutsideClick>
          <SheetTitle>Drawer</SheetTitle>
          <SheetDescription>Body</SheetDescription>
        </SheetContent>
      </Sheet>
    </WindowControlsContext.Provider>,
  );
  return { onOpenChange };
}

const getStrip = () =>
  document.querySelector<HTMLElement>("[data-tauri-drag-region]");

/** Radix 在下一帧才开始听外部 pointerdown：先让它挂上，再按 */
async function pressOutside(target: Element) {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
  await act(async () => {
    fireEvent.pointerDown(target, { button: 0, pointerType: "mouse" });
  });
}

describe("ModalDragStrip", () => {
  it("puts a drag region under the dialog so the overlay doesn't block moving the window", () => {
    renderDialog();
    const strip = getStrip()!;
    const dialog = screen.getByRole("dialog");
    expect(strip.parentElement).toHaveClass("fixed", "inset-0");
    // 遮罩排在弹窗内容前面，拖动条才不会挡住弹窗
    expect(
      strip.parentElement!.compareDocumentPosition(dialog) &
        Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    expect(dialog).not.toContainElement(strip);
  });

  it("leaves the overlay's drag region out where the platform disables dragging", () => {
    render(
      <Dialog open>
        <DialogContent>
          <DialogTitle>Title</DialogTitle>
          <DialogDescription>Body</DialogDescription>
        </DialogContent>
      </Dialog>,
    );
    expect(getStrip() !== null).toBe(DRAG_REGION_ENABLED);
  });

  it("doesn't dismiss click-outside dialogs when dragging the window", async () => {
    const onInteractOutside = vi.fn();
    renderDialog(onInteractOutside);
    const strip = getStrip()!;

    await pressOutside(strip);
    expect(onInteractOutside).not.toHaveBeenCalled();

    await pressOutside(strip.parentElement!);
    expect(onInteractOutside).toHaveBeenCalledOnce();
  });

  it("covers sheets, including ones dismissed by clicking the overlay", async () => {
    const { onOpenChange } = renderSheet();
    const strip = getStrip()!;
    expect(strip.parentElement).toHaveClass("fixed", "inset-0");

    await pressOutside(strip);
    expect(onOpenChange).not.toHaveBeenCalled();

    await pressOutside(strip.parentElement!);
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });
});
