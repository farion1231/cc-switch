import { fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { Popover, PopoverContent, PopoverTrigger } from "./popover";

function Example({ reducedMotion }: { reducedMotion?: boolean }) {
  const [open, setOpen] = useState(false);
  return (
    <Popover open={open} onOpenChange={setOpen} reducedMotion={reducedMotion}>
      <PopoverTrigger>Range</PopoverTrigger>
      <PopoverContent>Calendar</PopoverContent>
    </Popover>
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

describe("Popover animation guard", () => {
  beforeEach(() => {
    mockMotionPreference(false);
  });

  it("ignores another trigger click until the content animation ends", () => {
    render(<Example />);
    const trigger = screen.getByRole("button", { name: "Range" });

    fireEvent.click(trigger);
    const content = screen.getByText("Calendar");
    expect(trigger).toHaveAttribute("aria-expanded", "true");

    fireEvent.click(trigger);
    expect(trigger).toHaveAttribute("aria-expanded", "true");

    fireEvent.animationEnd(content);
    fireEvent.click(trigger);
    expect(trigger).toHaveAttribute("aria-expanded", "false");
  });

  it("does not lock or add motion classes when reduced motion is requested", () => {
    render(<Example reducedMotion />);
    const trigger = screen.getByRole("button", { name: "Range" });

    fireEvent.click(trigger);
    const content = screen.getByText("Calendar");
    expect(content).not.toHaveClass("data-[state=open]:animate-in");

    fireEvent.click(trigger);
    expect(trigger).toHaveAttribute("aria-expanded", "false");
  });

  it("uses the operating-system reduced-motion preference by default", () => {
    mockMotionPreference(true);
    render(<Example />);
    const trigger = screen.getByRole("button", { name: "Range" });

    fireEvent.click(trigger);
    fireEvent.click(trigger);

    expect(trigger).toHaveAttribute("aria-expanded", "false");
  });
});
