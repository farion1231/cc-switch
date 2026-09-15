import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { ModelDropdown } from "@/components/providers/forms/shared/ModelDropdown";

describe("ModelDropdown", () => {
  it("exposes a labelled search input and filters by vendor", async () => {
    const onSelect = vi.fn();
    Element.prototype.scrollIntoView = vi.fn();

    render(
      <ModelDropdown
        models={[
          { id: "gpt-5", ownedBy: "openai" },
          { id: "claude-sonnet", ownedBy: "anthropic" },
        ]}
        onSelect={onSelect}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Select model" }));

    const searchInput = screen.getByRole("combobox", {
      name: "Search models...",
    });
    expect(searchInput).toBeInTheDocument();

    fireEvent.change(searchInput, { target: { value: "openai" } });

    await waitFor(() => {
      expect(screen.getByRole("option", { name: "gpt-5" })).toBeVisible();
      expect(
        screen.queryByRole("option", { name: "claude-sonnet" }),
      ).not.toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole("option", { name: "gpt-5" }));
    expect(onSelect).toHaveBeenCalledWith("gpt-5");
  });
});
