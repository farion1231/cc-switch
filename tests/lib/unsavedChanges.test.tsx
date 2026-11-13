import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { FullScreenPanel } from "@/components/common/FullScreenPanel";
import { Sheet, SheetPageContent } from "@/components/ui/sheet";
import {
  discardUnsavedChanges,
  hasUnsavedChanges,
  useUnsavedChangesTracker,
} from "@/lib/unsavedChanges";

function Editor({ enabled = true }: { enabled?: boolean }) {
  return (
    <div {...useUnsavedChangesTracker(enabled)}>
      <input aria-label="name" />
      <button type="button" role="checkbox" aria-checked="false">
        claude
      </button>
      <button type="button" aria-pressed="false" data-unsaved-ignore>
        reveal
      </button>
      <button type="button">expand</button>
    </div>
  );
}

describe("unsaved changes tracker", () => {
  afterEach(() => discardUnsavedChanges());

  it("registers typing and edit controls, not view toggles or plain buttons", () => {
    render(<Editor />);
    fireEvent.click(screen.getByRole("button", { name: "reveal" }));
    fireEvent.click(screen.getByRole("button", { name: "expand" }));
    expect(hasUnsavedChanges()).toBe(false);

    fireEvent.click(screen.getByRole("checkbox", { name: "claude" }));
    expect(hasUnsavedChanges()).toBe(true);
  });

  it("registers text input and drops the entry on unmount", () => {
    const view = render(<Editor />);
    fireEvent.input(screen.getByRole("textbox", { name: "name" }), {
      target: { value: "x" },
    });
    expect(hasUnsavedChanges()).toBe(true);
    view.unmount();
    expect(hasUnsavedChanges()).toBe(false);
  });

  it("does nothing while disabled and forgets edits once disabled", () => {
    const view = render(<Editor enabled={false} />);
    fireEvent.input(screen.getByRole("textbox", { name: "name" }));
    expect(hasUnsavedChanges()).toBe(false);

    view.rerender(<Editor />);
    fireEvent.input(screen.getByRole("textbox", { name: "name" }));
    expect(hasUnsavedChanges()).toBe(true);
    view.rerender(<Editor enabled={false} />);
    expect(hasUnsavedChanges()).toBe(false);
  });

  it("tracks FullScreenPanel only when asked and only while open", () => {
    const panel = (props: { open: boolean; track: boolean }) => (
      <FullScreenPanel
        isOpen={props.open}
        title="Edit"
        onClose={() => undefined}
        trackUnsavedChanges={props.track}
      >
        <input aria-label="field" />
      </FullScreenPanel>
    );
    const view = render(panel({ open: true, track: false }));
    fireEvent.input(screen.getByRole("textbox", { name: "field" }));
    expect(hasUnsavedChanges()).toBe(false);

    view.rerender(panel({ open: true, track: true }));
    fireEvent.input(screen.getByRole("textbox", { name: "field" }));
    expect(hasUnsavedChanges()).toBe(true);
    view.rerender(panel({ open: false, track: true }));
    expect(hasUnsavedChanges()).toBe(false);
  });

  it("tracks sheet editor pages", () => {
    render(
      <Sheet open modal={false}>
        <SheetPageContent title="Edit MCP" closeLabel="back">
          <textarea aria-label="body" />
        </SheetPageContent>
      </Sheet>,
    );
    fireEvent.input(screen.getByRole("textbox", { name: "body" }));
    expect(hasUnsavedChanges()).toBe(true);
  });

  it("ignores filter inputs marked data-unsaved-ignore while tracking explicit edit buttons", () => {
    function FilterAndClearScope() {
      return (
        <div {...useUnsavedChangesTracker()}>
          <input aria-label="filter" data-unsaved-ignore />
          <button type="button" data-unsaved-edit>
            clear-icon
          </button>
        </div>
      );
    }

    render(<FilterAndClearScope />);
    fireEvent.input(screen.getByRole("textbox", { name: "filter" }), {
      target: { value: "claude" },
    });
    expect(hasUnsavedChanges()).toBe(false);

    fireEvent.click(screen.getByRole("button", { name: "clear-icon" }));
    expect(hasUnsavedChanges()).toBe(true);
  });

  it("gives edits in a nested sheet editor to that editor only", () => {
    function Outer({ showInner }: { showInner: boolean }) {
      return (
        <div {...useUnsavedChangesTracker()}>
          <input aria-label="provider" />
          {/* 外层豁免的即时操作区里打开的整页编辑器（portal 出去） */}
          <div data-unsaved-ignore>
            {showInner ? (
              <Sheet open modal={false}>
                <SheetPageContent title="Edit agent" closeLabel="back">
                  <textarea aria-label="prompt" />
                  <div data-unsaved-ignore>
                    <button type="button" aria-pressed="false">
                      view
                    </button>
                  </div>
                </SheetPageContent>
              </Sheet>
            ) : null}
          </div>
        </div>
      );
    }

    const view = render(<Outer showInner />);
    fireEvent.click(screen.getByRole("button", { name: "view" }));
    expect(hasUnsavedChanges()).toBe(false);

    fireEvent.input(screen.getByRole("textbox", { name: "prompt" }), {
      target: { value: "draft" },
    });
    fireEvent.change(screen.getByRole("textbox", { name: "prompt" }), {
      target: { value: "draft 2" },
    });
    expect(hasUnsavedChanges()).toBe(true);
    // 关掉子编辑器后草稿随之撤销，外层没有被算成有改动
    view.rerender(<Outer showInner={false} />);
    expect(hasUnsavedChanges()).toBe(false);

    // 外层本来就有的改动不会被内层撤掉
    fireEvent.input(screen.getByRole("textbox", { name: "provider" }));
    view.rerender(<Outer showInner />);
    fireEvent.input(screen.getByRole("textbox", { name: "prompt" }));
    view.rerender(<Outer showInner={false} />);
    expect(hasUnsavedChanges()).toBe(true);
  });
});
