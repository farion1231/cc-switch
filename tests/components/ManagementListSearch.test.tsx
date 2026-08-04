import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { ManagementListSearch } from "@/components/common/ManagementListSearch";

describe("ManagementListSearch", () => {
  it("reports input changes with an accessible search field", () => {
    const onValueChange = vi.fn();
    render(
      <ManagementListSearch
        value=""
        onValueChange={onValueChange}
        placeholder="Search managed items"
        ariaLabel="Search items"
        clearLabel="Clear search"
      />,
    );

    const input = screen.getByRole("textbox", { name: "Search items" });
    expect(input).toHaveAttribute("placeholder", "Search managed items");
    fireEvent.change(input, { target: { value: "alpha" } });
    expect(onValueChange).toHaveBeenCalledWith("alpha");
    expect(
      screen.queryByRole("button", { name: "Clear search" }),
    ).not.toBeInTheDocument();
  });

  it("clears a non-empty search from the button or Escape", () => {
    const onValueChange = vi.fn();
    render(
      <ManagementListSearch
        value="alpha"
        onValueChange={onValueChange}
        placeholder="Search managed items"
        ariaLabel="Search items"
        clearLabel="Clear search"
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Clear search" }));
    expect(onValueChange).toHaveBeenCalledWith("");

    fireEvent.keyDown(screen.getByRole("textbox", { name: "Search items" }), {
      key: "Escape",
    });
    expect(onValueChange).toHaveBeenLastCalledWith("");
  });
});
