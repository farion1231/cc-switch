import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { UsageInlineExtra } from "@/components/UsageFooter";

describe("UsageInlineExtra", () => {
  it("renders every usage window completely with muted clock icons", () => {
    const { container, getByText } = render(
      <UsageInlineExtra extra="5h:0%4h59m 7d:19%6d3h 30d:5%21d7h (ok) 未知" />,
    );

    expect(getByText("5h:")).toBeInTheDocument();
    expect(getByText("0%")).toBeInTheDocument();
    expect(getByText("4h59m")).toBeInTheDocument();
    expect(getByText("7d:")).toBeInTheDocument();
    expect(getByText("19%")).toBeInTheDocument();
    expect(getByText("6d3h")).toBeInTheDocument();
    expect(getByText("30d:")).toBeInTheDocument();
    expect(getByText("5%")).toBeInTheDocument();
    expect(getByText("21d7h")).toBeInTheDocument();
    expect(container.textContent).not.toContain("(ok)");
    expect(container.textContent).not.toContain("未知");

    const clocks = container.querySelectorAll("svg");
    expect(clocks).toHaveLength(3);
    for (const clock of clocks) {
      expect(clock.parentElement?.className).toContain(
        "text-muted-foreground/60",
      );
    }
    expect(container.innerHTML).not.toContain("truncate");
  });
});
