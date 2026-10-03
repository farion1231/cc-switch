import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { SessionMessageItem } from "@/components/sessions/SessionMessageItem";
import { TooltipProvider } from "@/components/ui/tooltip";

const renderMessage = (content: string, role = "tool", searchQuery?: string) =>
  render(
    <TooltipProvider>
      <SessionMessageItem
        message={{ role, content }}
        isActive={false}
        searchQuery={searchQuery}
        onCopy={vi.fn()}
      />
    </TooltipProvider>,
  );

const lines = (count: number, prefix = "line") =>
  Array.from({ length: count }, (_, i) => `${prefix} ${i}`).join("\n");

describe("SessionMessageItem tool message collapsing", () => {
  it("collapses a tool message to its first two lines", () => {
    const { container } = renderMessage(
      ["first line", "second line", "third line", "fourth line"].join("\n"),
    );

    expect(container).toHaveTextContent("first line");
    expect(container).toHaveTextContent("second line");
    expect(container).not.toHaveTextContent("third line");
  });

  it("keeps a short tool message intact", () => {
    const { container } = renderMessage("only one line");

    expect(container).toHaveTextContent("only one line");
    expect(screen.queryByRole("button", { name: /展开完整内容/ })).toBeNull();
  });

  it("keeps a tool message that only ends with a newline intact", () => {
    const { container } = renderMessage("ok\n");

    expect(container.textContent).not.toContain("…");
    expect(screen.queryByRole("button", { name: /展开完整内容/ })).toBeNull();
  });

  it("ignores blank tail lines when deciding to collapse", () => {
    renderMessage(["first line", "second line", "", "  ", ""].join("\n"));

    expect(screen.queryByRole("button", { name: /展开完整内容/ })).toBeNull();
  });

  it("ignores trailing blank lines in the line count", () => {
    renderMessage(`${lines(40)}\n\n`);

    expect(
      screen.getByRole("button", { name: /展开完整内容/ }),
    ).toHaveTextContent("40");
  });

  it("drops a trailing blank line from the preview", () => {
    const { container } = renderMessage(
      ["heading", "", "body line"].join("\n"),
    );

    expect(container.textContent).toContain("heading…");
  });

  it("caps an overlong single-line tool message by characters", () => {
    const { container } = renderMessage("x".repeat(1200));
    const body = container.querySelector(
      'div[class*="whitespace-pre-wrap"]',
    ) as HTMLElement;

    expect(body.textContent?.length).toBeLessThan(400);
    expect(
      screen.getByRole("button", { name: /展开完整内容/ }),
    ).toBeInTheDocument();
  });

  it("clamps the collapsed preview to two visual lines", () => {
    const { container } = renderMessage(lines(20));
    const body = container.querySelector('div[class*="whitespace-pre-wrap"]');

    expect(body?.className).toContain("line-clamp-2");
  });

  it("reports the line count instead of kilobytes", () => {
    renderMessage(lines(40));

    expect(
      screen.getByRole("button", { name: /展开完整内容/ }),
    ).toHaveTextContent("40");
  });

  it("expands a tool message to its full content on demand", async () => {
    const user = userEvent.setup();
    const { container } = renderMessage(
      ["first line", "second line", "third line"].join("\n"),
    );

    await user.click(screen.getByRole("button", { name: /展开完整内容/ }));

    expect(container).toHaveTextContent("third line");
    expect(
      container.querySelector('div[class*="whitespace-pre-wrap"]')?.className,
    ).not.toContain("line-clamp-2");
  });

  it("does not collapse a tool message whose hidden part matches the search", () => {
    const { container } = renderMessage(
      ["first line", "second line", "third line has needle"].join("\n"),
      "tool",
      "needle",
    );

    expect(container).toHaveTextContent("third line has needle");
    expect(screen.getByText("needle").tagName).toBe("MARK");
  });

  it("leaves assistant messages on the character threshold", () => {
    const { container } = renderMessage(
      lines(40, "assistant line"),
      "assistant",
    );

    expect(container).toHaveTextContent("assistant line 39");
    expect(screen.queryByRole("button", { name: /展开完整内容/ })).toBeNull();
  });

  it("still collapses a very long assistant message by characters", () => {
    const { container } = renderMessage("a".repeat(3200), "assistant");

    expect(container.textContent).toContain("…");
    expect(
      screen.getByRole("button", { name: /展开完整内容/ }),
    ).toHaveTextContent("3k");
  });

  it("leaves user messages untouched", () => {
    const { container } = renderMessage(lines(40, "user line"), "user");

    expect(container).toHaveTextContent("user line 39");
    expect(screen.queryByRole("button", { name: /展开完整内容/ })).toBeNull();
  });
});
