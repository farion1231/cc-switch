import { act, fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";

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
    expect(trigger).toHaveClass("data-[state=open]:pointer-events-auto");

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
});
