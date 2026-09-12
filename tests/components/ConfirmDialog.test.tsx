import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { ConfirmDialog } from "@/components/ConfirmDialog";

describe("ConfirmDialog", () => {
  it("prevents cancel and confirm actions while pending", () => {
    const onConfirm = vi.fn();
    const onCancel = vi.fn();

    render(
      <ConfirmDialog
        isOpen
        title="Confirm write"
        message="Please wait"
        pending
        onConfirm={onConfirm}
        onCancel={onCancel}
      />,
    );

    const cancelButton = screen.getByRole("button", { name: "common.cancel" });
    const confirmButton = screen.getByRole("button", {
      name: "common.confirm",
    });
    expect(cancelButton).toBeDisabled();
    expect(confirmButton).toBeDisabled();

    fireEvent.click(cancelButton);
    fireEvent.click(confirmButton);
    expect(onCancel).not.toHaveBeenCalled();
    expect(onConfirm).not.toHaveBeenCalled();
  });
});
