import { act, fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { HoverTip } from "@/components/ui/hover-tip";

function Example() {
  const [open, setOpen] = useState(false);
  return (
    <DropdownMenu open={open} onOpenChange={setOpen}>
      <DropdownMenuTrigger>Provider / Model</DropdownMenuTrigger>
      <DropdownMenuContent>
        <DropdownMenuItem>All providers</DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

describe("DropdownMenu animation guard", () => {
  beforeEach(() => {
    vi.stubGlobal("PointerEvent", MouseEvent);
  });

  it("ignores another pointer toggle for 200ms", async () => {
    render(<Example />);
    const trigger = screen.getByRole("button", { name: "Provider / Model" });

    fireEvent.pointerDown(trigger, { button: 0, ctrlKey: false });
    expect(screen.getByRole("menu")).toBeInTheDocument();
    expect(trigger).toHaveAttribute("aria-expanded", "true");

    await act(() => new Promise((resolve) => window.setTimeout(resolve, 0)));

    expect(document.body).toHaveStyle({ pointerEvents: "none" });
    expect(trigger).toHaveClass("aria-expanded:pointer-events-auto");

    fireEvent.pointerDown(trigger, { button: 0, ctrlKey: false });
    expect(trigger).toHaveAttribute("aria-expanded", "true");

    await act(() => new Promise((resolve) => window.setTimeout(resolve, 220)));
    fireEvent.pointerDown(trigger, { button: 0, ctrlKey: false });
    expect(trigger).toHaveAttribute("aria-expanded", "false");
  });

  it("ignores keyboard toggles during the lock window", async () => {
    render(<Example />);
    const trigger = screen.getByRole("button", { name: "Provider / Model" });

    fireEvent.keyDown(trigger, { key: "Enter" });
    expect(trigger).toHaveAttribute("aria-expanded", "true");

    fireEvent.keyDown(trigger, { key: "Enter" });
    expect(trigger).toHaveAttribute("aria-expanded", "true");

    await act(() => new Promise((resolve) => window.setTimeout(resolve, 220)));
    fireEvent.keyDown(trigger, { key: "Enter" });
    expect(trigger).toHaveAttribute("aria-expanded", "false");
  });

  it("keeps the trigger locked while an Escape dismissal finishes", async () => {
    render(<Example />);
    const trigger = screen.getByRole("button", { name: "Provider / Model" });

    fireEvent.pointerDown(trigger, { button: 0, ctrlKey: false });
    const content = screen.getByRole("menu");
    await act(() => new Promise((resolve) => window.setTimeout(resolve, 220)));

    fireEvent.keyDown(content, { key: "Escape" });
    expect(trigger).toHaveAttribute("aria-expanded", "false");

    fireEvent.pointerDown(trigger, { button: 0, ctrlKey: false });
    expect(trigger).toHaveAttribute("aria-expanded", "false");

    await act(() => new Promise((resolve) => window.setTimeout(resolve, 220)));
    fireEvent.pointerDown(trigger, { button: 0, ctrlKey: false });
    expect(trigger).toHaveAttribute("aria-expanded", "true");
  });

  it("keeps trigger pointer-events matching aria-expanded when wrapped in HoverTip", async () => {
    function ExampleWithHoverTip() {
      const [open, setOpen] = useState(false);
      return (
        <DropdownMenu open={open} onOpenChange={setOpen}>
          <HoverTip content="More actions">
            <DropdownMenuTrigger asChild>
              <button type="button">More</button>
            </DropdownMenuTrigger>
          </HoverTip>
          <DropdownMenuContent>
            <DropdownMenuItem>Edit</DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      );
    }

    render(<ExampleWithHoverTip />);
    const trigger = screen.getByRole("button", { name: "More" });

    fireEvent.pointerDown(trigger, { button: 0, ctrlKey: false });
    expect(screen.getByRole("menu")).toBeInTheDocument();
    expect(trigger).toHaveAttribute("aria-expanded", "true");
    // HoverTip's TooltipTrigger overrides data-state, so pointer-events must rely on aria-expanded
    expect(trigger).toHaveAttribute("data-state", "closed");
    expect(trigger).toHaveClass("aria-expanded:pointer-events-auto");

    fireEvent.pointerDown(trigger, { button: 0, ctrlKey: false });
    expect(trigger).toHaveAttribute("aria-expanded", "true");

    await act(() => new Promise((resolve) => window.setTimeout(resolve, 220)));
    fireEvent.pointerDown(trigger, { button: 0, ctrlKey: false });
    expect(trigger).toHaveAttribute("aria-expanded", "false");
  });

  it("does not block non-toggle keys or secondary pointer clicks during the lock window", async () => {
    render(<Example />);
    const trigger = screen.getByRole("button", { name: "Provider / Model" });

    fireEvent.pointerDown(trigger, { button: 0, ctrlKey: false });
    const content = screen.getByRole("menu");
    await act(() => new Promise((resolve) => window.setTimeout(resolve, 220)));

    fireEvent.keyDown(content, { key: "Escape" });
    expect(trigger).toHaveAttribute("aria-expanded", "false");

    const tabEvent = new KeyboardEvent("keydown", {
      key: "Tab",
      bubbles: true,
      cancelable: true,
    });
    trigger.dispatchEvent(tabEvent);
    expect(tabEvent.defaultPrevented).toBe(false);

    const contextClick = new MouseEvent("pointerdown", {
      button: 2,
      bubbles: true,
      cancelable: true,
    });
    trigger.dispatchEvent(contextClick);
    expect(contextClick.defaultPrevented).toBe(false);
  });
});
