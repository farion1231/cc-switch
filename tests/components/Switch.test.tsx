import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { Switch } from "@/components/ui/switch";

describe("Switch", () => {
  it("keeps the default size and action color", () => {
    render(<Switch aria-label="default" defaultChecked />);
    const element = screen.getByRole("switch", { name: "default" });
    expect(element).toHaveAttribute("data-size", "default");
    expect(element.className).toContain("w-[30px]");
    expect(element.className).toContain("data-[state=checked]:bg-action");
  });

  it("uses the action color for the small variant too", () => {
    render(<Switch aria-label="small" size="sm" />);
    const element = screen.getByRole("switch", { name: "small" });
    expect(element).toHaveAttribute("data-size", "sm");
    expect(element.className).toContain("w-[26px]");
    expect(element.className).toContain("data-[state=checked]:bg-action");
  });
});
