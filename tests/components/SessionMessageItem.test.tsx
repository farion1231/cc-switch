import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
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

  it("renders every line in an indented code block", () => {
    const { container } = renderMessage("    first\n    second");

    expect(container.querySelector("code")).toHaveTextContent("first\nsecond", {
      normalizeWhitespace: false,
    });
  });

  it("resolves reference-style links through their definitions", () => {
    const { container } = renderMessage(
      '[docs][reference]\n\n[reference]: https://example.com "Documentation"',
    );

    expect(screen.getByRole("link", { name: "docs" })).toHaveAttribute(
      "href",
      "https://example.com",
    );
    expect(container).not.toHaveTextContent("[reference]:");
  });

  it("removes subscript and superscript delimiter markers", () => {
    const { container } = renderMessage("H~2~O and x^2^");

    expect(container.querySelector("sub")).toHaveTextContent("2");
    expect(container.querySelector("sup")).toHaveTextContent("2");
    expect(container).not.toHaveTextContent("~2~");
    expect(container).not.toHaveTextContent("^2^");
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

  it("does not request remote images until the user chooses to load them", async () => {
    const user = userEvent.setup();
    const { container } = renderMessage(
      "![tracking pixel](https://tracker.example/unique-id)",
    );

    expect(container.querySelector("img")).toBeNull();

    await user.click(screen.getByRole("button", { name: /tracking pixel/ }));

    expect(screen.getByRole("img", { name: "tracking pixel" })).toHaveAttribute(
      "src",
      "https://tracker.example/unique-id",
    );
  });

  it("requires new consent when a rendered remote image URL changes", async () => {
    const user = userEvent.setup();
    const { container, rerender } = render(
      <SessionMarkdown content="![first](https://tracker.example/first)" />,
    );

    await user.click(screen.getByRole("button", { name: /first/ }));
    expect(screen.getByRole("img", { name: "first" })).toHaveAttribute(
      "src",
      "https://tracker.example/first",
    );

    rerender(
      <SessionMarkdown content="![second](https://tracker.example/second)" />,
    );

    expect(container.querySelector("img")).toBeNull();
    expect(screen.getByRole("button", { name: /second/ })).toBeInTheDocument();
  });

  it("renders table rows using only semantic cell elements", () => {
    const { container } = renderMessage(
      ["| Name | Status |", "| --- | --- |", "| CC Switch | Ready |"].join(
        "\n",
      ),
    );
    const rows = Array.from(container.querySelectorAll("tr"));

    expect(rows).toHaveLength(2);
    expect(Array.from(rows[0].children).map((cell) => cell.tagName)).toEqual([
      "TH",
      "TH",
    ]);
    expect(Array.from(rows[1].children).map((cell) => cell.tagName)).toEqual([
      "TD",
      "TD",
    ]);
    rows.forEach((row) => expect(row).not.toHaveTextContent("|"));
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

  it("shows search context when a match is inside collapsed content", () => {
    renderMessage(
      `# Result\n\n${"a".repeat(1800)}needle${"b".repeat(1800)}`,
      "needle",
    );

    expect(screen.getByText("needle").tagName).toBe("MARK");
    expect(screen.getByText("原文中的匹配")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /展开完整内容/ }),
    ).toHaveAttribute("aria-expanded", "false");
  });

  it("shows search context when a match crosses the collapse boundary", () => {
    renderMessage(`${"a".repeat(1497)}needle${"b".repeat(1600)}`, "needle");

    expect(screen.getByText("needle").tagName).toBe("MARK");
    expect(screen.getByText("原文中的匹配")).toBeInTheDocument();
  });

  it("shows search context when the match only exists in a hidden link URL", () => {
    renderMessage("See [docs](https://hidden-domain.example/page).", "hidden-domain");

    expect(screen.getByText("hidden-domain").tagName).toBe("MARK");
    expect(screen.getByText("原文中的匹配")).toBeInTheDocument();
  });

  it("does not show a snippet when the match is visible in rendered Markdown", () => {
    renderMessage("The **important result** is ready.", "result");

    expect(screen.getByText("result").tagName).toBe("MARK");
    expect(screen.queryByText("原文中的匹配")).toBeNull();
  });

  it("closes a truncated code fence before rendering the preview ellipsis", () => {
    const { container } = renderMessage(
      [
        "Before",
        "",
        "```ts",
        `const value = "${"x".repeat(3200)}";`,
        "```",
      ].join("\n"),
    );
    const codeBlock = container.querySelector("pre");

    expect(codeBlock).not.toBeNull();
    expect(codeBlock).not.toHaveTextContent("…");
    expect(container).toHaveTextContent("…");
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
