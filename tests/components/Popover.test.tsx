import { act, fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { describe, expect, it } from "vitest";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";

function Example() {
  const [open, setOpen] = useState(false);
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger>Range</PopoverTrigger>
      <PopoverContent>Calendar</PopoverContent>
    </Popover>
  );
}

describe("Popover animation guard", () => {
  it("ignores trigger clicks for 200ms after a toggle", async () => {
    render(<Example />);
    const trigger = screen.getByRole("button", { name: "Range" });

    fireEvent.click(trigger);
    expect(screen.getByText("Calendar")).toBeInTheDocument();
    expect(trigger).toHaveAttribute("aria-expanded", "true");

    fireEvent.click(trigger);
    expect(trigger).toHaveAttribute("aria-expanded", "true");

    await act(() => new Promise((resolve) => window.setTimeout(resolve, 220)));
    fireEvent.click(trigger);
    expect(trigger).toHaveAttribute("aria-expanded", "false");
  });
});
