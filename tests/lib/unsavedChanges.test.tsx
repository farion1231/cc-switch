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
});
