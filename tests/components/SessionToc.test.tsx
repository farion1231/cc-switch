import { useState } from "react";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { SessionTocDialog } from "@/components/sessions/SessionToc";

const items = [
  { index: 0, preview: "First question" },
  { index: 2, preview: "Second question" },
  { index: 4, preview: "Third question" },
];

describe("SessionTocDialog", () => {
  it("escapes the transformed page while preserving dialog interaction and focus", async () => {
    const onItemClick = vi.fn();
    function Page() {
      const [open, setOpen] = useState(false);
      return (
        <div data-testid="page" style={{ transform: "translateY(10px)" }}>
          <SessionTocDialog
            items={items}
            open={open}
            onOpenChange={setOpen}
            onItemClick={(index) => {
              onItemClick(index);
              setOpen(false);
            }}
          />
        </div>
      );
    }

    const user = userEvent.setup();
    const { unmount } = render(<Page />);
    const trigger = screen.getByRole("button");
    // A transformed ancestor would re-anchor a fixed trigger during page entry.
    expect(screen.getByTestId("page")).not.toContainElement(trigger);

    await user.click(trigger);
    expect(screen.getByRole("dialog")).toBeVisible();
    await user.click(screen.getByRole("button", { name: /Second question/ }));
    expect(onItemClick).toHaveBeenCalledWith(2);
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    await waitFor(() => expect(trigger).toHaveFocus());

    await user.click(trigger);
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    await waitFor(() => expect(trigger).toHaveFocus());

    // Navigating away while open must remove the portal and the modal together.
    await user.click(trigger);
    unmount();
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(trigger).not.toBeInTheDocument();
  });

  it("removes the floating trigger when the selected session has too few entries", () => {
    const props = {
      open: false,
      onOpenChange: vi.fn(),
      onItemClick: vi.fn(),
    };
    const { rerender } = render(<SessionTocDialog {...props} items={items} />);
    const trigger = screen.getByRole("button");
    rerender(<SessionTocDialog {...props} items={items.slice(0, 2)} />);
    expect(trigger).not.toBeInTheDocument();
    expect(screen.queryByRole("button")).toBeNull();
  });
});
