import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { AppToggleGroup } from "@/components/common/AppToggleGroup";
import { TooltipProvider } from "@/components/ui/tooltip";

describe("AppToggleGroup", () => {
  it("exposes each app state and respects the shared disabled state", () => {
    const onToggle = vi.fn();
    const { rerender } = render(
      <TooltipProvider>
        <AppToggleGroup
          apps={{ claude: true }}
          appIds={["claude"]}
          onToggle={onToggle}
        />
      </TooltipProvider>,
    );

    const enabledButton = screen.getByRole("button", { name: "Claude" });
    expect(enabledButton).toHaveAttribute("aria-pressed", "true");
    fireEvent.click(enabledButton);
    expect(onToggle).toHaveBeenCalledWith("claude", false);

    rerender(
      <TooltipProvider>
        <AppToggleGroup
          apps={{ claude: false }}
          appIds={["claude"]}
          onToggle={onToggle}
          disabled
        />
      </TooltipProvider>,
    );

    const disabledButton = screen.getByRole("button", { name: "Claude" });
    expect(disabledButton).toHaveAttribute("aria-pressed", "false");
    expect(disabledButton).toBeDisabled();
    expect(disabledButton.className).not.toContain("disabled:opacity-");
  });
});
