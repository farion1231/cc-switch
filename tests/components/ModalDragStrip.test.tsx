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

function renderDialog(onInteractOutside?: () => void, onMinimize?: () => void) {
  return render(
    <WindowControlsContext.Provider
      value={<button onClick={onMinimize}>Minimize</button>}
    >
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

  it("keeps the window controls usable while a dialog is open", async () => {
    const onInteractOutside = vi.fn();
    const onMinimize = vi.fn();
    renderDialog(onInteractOutside, onMinimize);
    const strip = getStrip()!;
    // 遮罩被 Radix 标了 aria-hidden，按名字查要带 hidden
    const minimize = screen.getByRole("button", {
      name: "Minimize",
      hidden: true,
    });
    expect(strip).toContainElement(minimize);
    // Tauri 只认按下的元素自身带属性：按钮上按下去不能变成拖窗口
    expect(minimize).not.toHaveAttribute("data-tauri-drag-region");

    await pressOutside(minimize);
    // 返回 false 说明默认动作被拦了：焦点留在弹窗里，不算「点外面」
    expect(fireEvent.mouseDown(minimize)).toBe(false);
    fireEvent.click(minimize);

    expect(onMinimize).toHaveBeenCalledOnce();
    expect(onInteractOutside).not.toHaveBeenCalled();
    expect(screen.getByRole("dialog")).toBeInTheDocument();
  });

  it("reaches down to the page header when a banner pushes it below the top", () => {
    const header = document.createElement("header");
    vi.spyOn(header, "getBoundingClientRect").mockReturnValue({
      bottom: 119,
    } as DOMRect);
    const main = document.createElement("main");
    main.id = "content-area";
    main.append(header);
    document.body.append(main);
    try {
      renderDialog();
      expect(getStrip()).toHaveStyle({ height: "119px" });
    } finally {
      main.remove();
    }
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
