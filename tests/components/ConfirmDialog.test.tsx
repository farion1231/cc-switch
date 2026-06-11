import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { ConfirmDialog } from "@/components/ConfirmDialog";

// Return i18n keys as-is for stable assertions
vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}));

describe("ConfirmDialog", () => {
  it("does not render a checkbox when the checkbox prop is omitted", () => {
    render(
      <ConfirmDialog
        isOpen
        title="Delete provider"
        message="Are you sure?"
        onConfirm={() => {}}
        onCancel={() => {}}
      />,
    );

    expect(screen.queryByRole("checkbox")).toBeNull();
  });

  it("renders the checkbox with its label and toggles via onChange", () => {
    const onChange = vi.fn();

    render(
      <ConfirmDialog
        isOpen
        title="Enable routing"
        message="Routing is required."
        checkbox={{
          label: "Remember my choice",
          checked: false,
          onChange,
        }}
        onConfirm={() => {}}
        onCancel={() => {}}
      />,
    );

    const checkbox = screen.getByRole("checkbox");
    expect(checkbox).toBeInTheDocument();
    expect(screen.getByText("Remember my choice")).toBeInTheDocument();

    fireEvent.click(checkbox);
    expect(onChange).toHaveBeenCalledWith(true);
  });
});
