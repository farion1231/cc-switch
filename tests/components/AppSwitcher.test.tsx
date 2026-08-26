import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { AppSwitcher } from "@/components/AppSwitcher";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

describe("AppSwitcher", () => {
  it("renders the Cursor brand icon", () => {
    render(
      <div>
        <AppSwitcher activeApp="cursor" onSwitch={vi.fn()} />
      </div>,
    );

    const cursorButton = screen.getByRole("button", { name: "Cursor" });
    const cursorIcon = screen.getByRole("img", { name: "Cursor" });

    expect(cursorButton).toContainElement(cursorIcon);
    expect(cursorIcon).toHaveAttribute(
      "src",
      expect.stringContaining("cursor"),
    );
  });
});
