import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { Switch } from "@/components/ui/switch";

describe("Switch", () => {
  it("keeps the default size and action tone", () => {
    render(<Switch aria-label="default" defaultChecked />);
    const element = screen.getByRole("switch", { name: "default" });
    expect(element).toHaveAttribute("data-size", "default");
    expect(element).toHaveAttribute("data-tone", "action");
    expect(element.className).toContain("w-[30px]");
    expect(element.className).toContain("data-[state=checked]:bg-action");
  });

  it("supports a small neutral variant", () => {
    render(<Switch aria-label="small" size="sm" tone="neutral" />);
    const element = screen.getByRole("switch", { name: "small" });
    expect(element).toHaveAttribute("data-size", "sm");
    expect(element).toHaveAttribute("data-tone", "neutral");
    expect(element.className).toContain("w-[26px]");
    expect(element.className).toContain("data-[state=checked]:bg-fg-2");
    expect(element.className).not.toContain("data-[state=checked]:bg-action");
  });
});
