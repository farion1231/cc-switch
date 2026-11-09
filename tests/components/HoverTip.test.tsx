import { act, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { HoverTip } from "@/components/ui/hover-tip";

function renderTip() {
  render(
    <HoverTip content="更多操作">
      <button type="button" aria-label="更多">
        ⋯
      </button>
    </HoverTip>,
  );
  return screen.getByRole("button", { name: "更多" });
}

describe("HoverTip", () => {
  afterEach(() => {
    delete document.documentElement.dataset.keyboard;
  });

  it("stays closed when focus returns to the trigger without the keyboard (menu closed by click)", () => {
    const button = renderTip();
    act(() => button.focus());
    expect(screen.queryByRole("tooltip")).not.toBeInTheDocument();
  });

  it("opens on keyboard focus", () => {
    const button = renderTip();
    document.documentElement.dataset.keyboard = "";
    act(() => button.focus());
    expect(screen.getByRole("tooltip")).toHaveTextContent("更多操作");
  });
});
