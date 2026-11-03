import { act, fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "./dropdown-menu";

function Example({ reducedMotion }: { reducedMotion?: boolean }) {
  const [open, setOpen] = useState(false);
  return (
    <DropdownMenu
      open={open}
      onOpenChange={setOpen}
      reducedMotion={reducedMotion}
    >
      <DropdownMenuTrigger>Provider / Model</DropdownMenuTrigger>
      <DropdownMenuContent>
        <DropdownMenuItem>All providers</DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

function mockMotionPreference(matches: boolean) {
  vi.stubGlobal(
    "matchMedia",
    vi.fn().mockReturnValue({
      matches,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    }),
  );
}

describe("DropdownMenu animation guard", () => {
  beforeEach(() => {
    mockMotionPreference(false);
    vi.stubGlobal("PointerEvent", MouseEvent);
  });

  it("ignores another pointer toggle until the content animation ends", async () => {
    render(<Example />);
    const trigger = screen.getByRole("button", { name: "Provider / Model" });

    fireEvent.pointerDown(trigger, { button: 0, ctrlKey: false });
    const content = screen.getByRole("menu");
    expect(trigger).toHaveAttribute("aria-expanded", "true");

    // Radix registers its document-level outside-pointer listener after mount.
    await act(() => new Promise((resolve) => window.setTimeout(resolve, 0)));

    fireEvent.pointerDown(trigger, { button: 0, ctrlKey: false });
    expect(trigger).toHaveAttribute("aria-expanded", "true");

    fireEvent.animationEnd(content);
    fireEvent.pointerDown(trigger, { button: 0, ctrlKey: false });
    expect(trigger).toHaveAttribute("aria-expanded", "false");
  });

  it("does not lock or add motion classes with reduced motion", () => {
    render(<Example reducedMotion />);
    const trigger = screen.getByRole("button", { name: "Provider / Model" });

    fireEvent.pointerDown(trigger, { button: 0, ctrlKey: false });
    expect(screen.getByRole("menu")).not.toHaveClass(
      "data-[state=open]:animate-in",
    );

    fireEvent.pointerDown(trigger, { button: 0, ctrlKey: false });
    expect(trigger).toHaveAttribute("aria-expanded", "false");
  });
});
