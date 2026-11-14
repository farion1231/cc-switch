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

  it("starts sheets below the strip so the drawer doesn't cover the window controls", () => {
    // 页头上方有警告条时也得接得上：抽屉顶边要和拖动条下沿是同一个量出来的值
    const header = document.createElement("header");
    vi.spyOn(header, "getBoundingClientRect").mockReturnValue({
      bottom: 119,
    } as DOMRect);
    const main = document.createElement("main");
    main.id = "content-area";
    main.append(header);
    document.body.append(main);
    try {
      renderSheet();
      const strip = getStrip()!;
      const sheet = screen.getByRole("dialog");
      const minimize = screen.getByRole("button", {
        name: "Minimize",
        hidden: true,
      });
      // 抽屉和遮罩同一层 z-index、DOM 里排在后面，重叠处一定是抽屉在上：
      // 窗口按钮必须在拖动条里、拖动条必须整个落在抽屉顶边以上
      expect(strip).toContainElement(minimize);
      expect(sheet).not.toContainElement(minimize);
      expect(strip).toHaveStyle({ height: "119px" });
      expect(sheet).toHaveStyle({ top: "119px" });
    } finally {
      main.remove();
    }
  });

  it("re-measures the header when a mounted-but-closed sheet opens", () => {
    // 路由设置这类抽屉随页面常驻、只切换 open；警告条是异步检查完才出现的，
    // 抽屉挂载时量到的值到打开时已经过时，得在打开时重新量
    const header = document.createElement("header");
    const rect = vi
      .spyOn(header, "getBoundingClientRect")
      .mockReturnValue({ bottom: 52 } as DOMRect);
    const main = document.createElement("main");
    main.id = "content-area";
    main.append(header);
    document.body.append(main);
    const drawer = (open: boolean) => (
      <WindowControlsContext.Provider value={<button>Minimize</button>}>
        <Sheet open={open}>
          <SheetContent closeLabel="close">
            <SheetTitle>Drawer</SheetTitle>
            <SheetDescription>Body</SheetDescription>
          </SheetContent>
        </Sheet>
      </WindowControlsContext.Provider>
    );
    try {
      const { rerender } = render(drawer(false));
      expect(screen.queryByRole("dialog")).toBeNull();

      // 关着的时候警告条出现，页头被推下去
      rect.mockReturnValue({ bottom: 119 } as DOMRect);
      rerender(drawer(true));
      expect(getStrip()).toHaveStyle({ height: "119px" });
      expect(screen.getByRole("dialog")).toHaveStyle({ top: "119px" });

      // 反过来：关掉警告条后再打开，抽屉顶边也跟着回到 52，不留空隙
      rerender(drawer(false));
      rect.mockReturnValue({ bottom: 52 } as DOMRect);
      rerender(drawer(true));
      expect(getStrip()).toHaveStyle({ height: "52px" });
      expect(screen.getByRole("dialog")).toHaveStyle({ top: "52px" });
    } finally {
      main.remove();
    }
  });

  it("keeps sheets full height when there are no in-app window controls", () => {
    render(
      <Sheet open>
        <SheetContent closeLabel="close">
          <SheetTitle>Drawer</SheetTitle>
          <SheetDescription>Body</SheetDescription>
        </SheetContent>
      </Sheet>,
    );
    expect(screen.getByRole("dialog").style.top).toBe("");
  });
});
