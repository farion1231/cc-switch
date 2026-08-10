import { render, screen } from "@testing-library/react";
import { markdownLanguage } from "@codemirror/lang-markdown";
import { describe, expect, it, vi } from "vitest";

import { SessionMessageItem } from "@/components/sessions/SessionMessageItem";
import { SessionMarkdown } from "@/components/sessions/SessionMarkdown";
import { TooltipProvider } from "@/components/ui/tooltip";

const renderMessage = (
  content: string,
  searchQuery?: string,
  role = "assistant",
) =>
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

describe("SessionMessageItem", () => {
  it("renders common Markdown structures instead of showing their markers", () => {
    const { container } = renderMessage(
      [
        "## Result",
        "",
        "Use **safe mode** with `cargo test`.",
        "",
        "- first",
        "- second",
        "",
        "> quoted",
        "",
        "[docs](https://example.com) and <user@example.com>",
        "",
        "```ts",
        "const ready = true;",
        "```",
      ].join("\n"),
    );

    expect(
      screen.getByRole("heading", { level: 2, name: "Result" }),
    ).toBeInTheDocument();
    expect(screen.getByText("safe mode").tagName).toBe("STRONG");
    expect(screen.getByText("cargo test").tagName).toBe("CODE");
    expect(screen.getByRole("list")).toBeInTheDocument();
    expect(screen.getByText("quoted").closest("blockquote")).not.toBeNull();
    expect(screen.getByRole("link", { name: "docs" })).toHaveAttribute(
      "href",
      "https://example.com",
    );
    expect(
      screen.getByRole("link", { name: "user@example.com" }),
    ).toHaveAttribute("href", "mailto:user@example.com");
    expect(screen.getByText("const ready = true;").tagName).toBe("CODE");
    expect(container).not.toHaveTextContent("## Result");
    expect(container).not.toHaveTextContent("**safe mode**");
  });

  it("keeps search matches highlighted inside rendered Markdown", () => {
    renderMessage("The **important result** is ready.", "result");

    expect(screen.getByText("result").tagName).toBe("MARK");
    expect(screen.getByText(/important/).tagName).toBe("STRONG");
  });

  it("does not turn raw HTML or unsafe links into executable markup", () => {
    const { container } = renderMessage(
      '<script>alert("xss")</script> [unsafe](javascript:alert(1))',
    );

    expect(container.querySelector("script")).toBeNull();
    expect(screen.queryByRole("link", { name: "unsafe" })).toBeNull();
    expect(container).toHaveTextContent('<script>alert("xss")</script>');
  });

  it("keeps non-assistant messages in fast plain-text mode", () => {
    const { container } = renderMessage(
      "## Context\n\nKeep **literal markers** here.",
      undefined,
      "developer",
    );

    expect(container.querySelector("h2")).toBeNull();
    expect(container.querySelector("strong")).toBeNull();
    expect(container).toHaveTextContent("## Context");
    expect(container).toHaveTextContent("**literal markers**");
  });

  it("keeps long messages collapsed even when the search matches hidden text", () => {
    renderMessage(
      `# Result\n\n${"a".repeat(1800)}needle${"b".repeat(1800)}`,
      "needle",
    );

    expect(screen.queryByText("needle")).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /展开完整内容/ }),
    ).toHaveAttribute("aria-expanded", "false");
  });

  it("does not reparse Markdown when only search highlighting changes", () => {
    const parseSpy = vi.spyOn(markdownLanguage.parser, "parse");

    try {
      const { rerender } = render(<SessionMarkdown content="**stable**" />);

      rerender(<SessionMarkdown content="**stable**" searchQuery="stable" />);

      expect(parseSpy).toHaveBeenCalledTimes(1);
    } finally {
      parseSpy.mockRestore();
    }
  });
});
