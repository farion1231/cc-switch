// 底本：farion1231/cc-switch#6332 的 tests/components/SessionMessageItem.test.tsx。
// 迁入后直接测 SessionMarkdown；原先依赖旧 SessionMessageItem 的折叠与
// 「原文中的匹配」用例改为断言导出的 createCollapsedMarkdownPreview /
// hasHighlightableMarkdownMatch（片段展示由阅读页组件负责）。
import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { markdownLanguage } from "@codemirror/lang-markdown";
import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  createCollapsedMarkdownPreview,
  hasHighlightableMarkdownMatch,
  MARKDOWN_PARSE_LIMIT,
  SessionMarkdown,
  SessionPlainText,
} from "@/components/sessions/reader/SessionMarkdown";

const { invokeMock, copyTextMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  copyTextMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
vi.mock("@/lib/clipboard", () => ({ copyText: copyTextMock }));

const COLLAPSED_LENGTH = 1500;

const renderMarkdown = (content: string, searchQuery?: string) =>
  render(<SessionMarkdown content={content} searchQuery={searchQuery} />);

const renderCollapsed = (content: string, searchQuery?: string) =>
  renderMarkdown(
    createCollapsedMarkdownPreview(content, COLLAPSED_LENGTH),
    searchQuery,
  );

beforeEach(() => {
  invokeMock.mockReset();
  invokeMock.mockResolvedValue(undefined);
  copyTextMock.mockReset();
  copyTextMock.mockResolvedValue(undefined);
});

describe("SessionMarkdown", () => {
  it("renders common Markdown structures instead of showing their markers", () => {
    const { container } = renderMarkdown(
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

  it("uses v7 design tokens for text, code, links and headings", () => {
    const { container } = renderMarkdown(
      [
        "# Title",
        "",
        "## Section",
        "",
        "### Sub",
        "",
        "Run `cargo test` and see [docs](https://example.com).",
        "",
        "> quoted",
      ].join("\n"),
    );

    const root = container.querySelector("[data-session-markdown]");
    expect(root).toHaveClass("text-body", "text-fg-1");
    expect(screen.getByRole("heading", { level: 1 })).toHaveClass("text-title");
    expect(screen.getByRole("heading", { level: 2 })).toHaveClass(
      "text-section",
    );
    expect(screen.getByRole("heading", { level: 3 })).toHaveClass(
      "text-body",
      "font-semibold",
    );
    expect(screen.getByText("cargo test")).toHaveClass(
      "bg-subtle",
      "border-border",
      "font-mono",
    );
    expect(screen.getByRole("link", { name: "docs" })).toHaveClass(
      "text-action-text",
    );
    expect(screen.getByText("quoted").closest("blockquote")).toHaveClass(
      "text-fg-2",
    );
    expect(container.innerHTML).not.toMatch(
      /text-muted-foreground|text-primary/,
    );
  });

  it("renders the note variant with caption text", () => {
    const { container } = render(
      <SessionMarkdown content="**note**" variant="note" />,
    );

    expect(container.querySelector("[data-session-markdown]")).toHaveClass(
      "text-caption",
      "text-fg-2",
    );
  });

  it("renders every line in an indented code block", () => {
    const { container } = renderMarkdown("    first\n    second");

    expect(container.querySelector("code")).toHaveTextContent("first\nsecond", {
      normalizeWhitespace: false,
    });
  });

  it("renders fenced code with a language label and a copy button", async () => {
    const user = userEvent.setup();
    renderMarkdown("```rust\nfn main() {}\n```");

    expect(screen.getByText("rust")).toBeInTheDocument();
    expect(screen.getByText("fn main() {}").closest("pre")).not.toBeNull();

    await user.click(screen.getByRole("button", { name: "复制代码" }));

    expect(copyTextMock).toHaveBeenCalledWith("fn main() {}");
  });

  it("resolves reference-style links through their definitions", () => {
    const { container } = renderMarkdown(
      '[docs][reference]\n\n[reference]: https://example.com "Documentation"',
    );

    expect(screen.getByRole("link", { name: "docs" })).toHaveAttribute(
      "href",
      "https://example.com",
    );
    expect(container).not.toHaveTextContent("[reference]:");
  });

  it("resolves shortcut references using their source label", () => {
    renderMarkdown(
      "[**formatted docs**]\n\n[**formatted docs**]: https://example.com/formatted",
    );

    expect(
      screen.getByRole("link", { name: "formatted docs" }),
    ).toHaveAttribute("href", "https://example.com/formatted");
  });

  it("removes subscript and superscript delimiter markers", () => {
    const { container } = renderMarkdown("H~2~O and x^2^");

    expect(container.querySelector("sub")).toHaveTextContent("2");
    expect(container.querySelector("sup")).toHaveTextContent("2");
    expect(container).not.toHaveTextContent("~2~");
    expect(container).not.toHaveTextContent("^2^");
  });

  it("keeps search matches highlighted inside rendered Markdown", () => {
    renderMarkdown("The **important result** is ready.", "result");

    expect(screen.getByText("result").tagName).toBe("MARK");
    expect(screen.getByText(/important/).tagName).toBe("STRONG");
  });

  it("does not turn raw HTML or unsafe links into executable markup", () => {
    const { container } = renderMarkdown(
      '<script>alert("xss")</script> [unsafe](javascript:alert(1))',
    );

    expect(container.querySelector("script")).toBeNull();
    expect(screen.queryByRole("link", { name: "unsafe" })).toBeNull();
    expect(container).toHaveTextContent('<script>alert("xss")</script>');
  });

  it("does not request remote images until the user chooses to load them", async () => {
    const user = userEvent.setup();
    const { container } = renderMarkdown(
      "![tracking pixel](https://tracker.example/unique-id)",
    );

    expect(container.querySelector("img")).toBeNull();

    await user.click(screen.getByRole("button", { name: /tracking pixel/ }));

    const image = screen.getByRole("img", { name: "tracking pixel" });
    expect(image).toHaveAttribute("src", "https://tracker.example/unique-id");
    expect(image).toHaveAttribute("loading", "lazy");
    expect(image).toHaveClass("max-h-80", "max-w-full");
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

  it("loads remote images lazily when auto-loading is enabled and falls back to a link on error", () => {
    const onOpenLink = vi.fn();
    render(
      <SessionMarkdown
        content="![chart](https://example.com/chart.png)"
        autoLoadRemoteImages
        onOpenLink={onOpenLink}
      />,
    );

    const image = screen.getByRole("img", { name: "chart" });
    expect(image).toHaveAttribute("loading", "lazy");

    fireEvent.error(image);

    expect(screen.queryByRole("img")).toBeNull();
    const link = screen.getByRole("link", { name: "chart" });
    expect(link).toHaveAttribute("title", "https://example.com/chart.png");
    fireEvent.click(link);
    expect(onOpenLink).toHaveBeenCalledWith("https://example.com/chart.png");
  });

  it("renders table rows using only semantic cell elements", () => {
    const { container } = renderMarkdown(
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

  it("keeps plain text mode free of Markdown rendering", () => {
    const { container } = render(
      <SessionPlainText
        content={
          "## Context\n\nKeep **literal markers** here.\n\n```sh\nls -la\n```"
        }
      />,
    );

    expect(container.querySelector("h2")).toBeNull();
    expect(container.querySelector("strong")).toBeNull();
    expect(container).toHaveTextContent("## Context");
    expect(container).toHaveTextContent("**literal markers**");
    expect(screen.getByText("ls -la").closest("pre")).not.toBeNull();
    expect(screen.getByText("sh")).toBeInTheDocument();
  });

  it("hides a match that lies entirely inside collapsed content", () => {
    const content = `# Result\n\n${"a".repeat(1800)}needle${"b".repeat(1800)}`;
    const preview = createCollapsedMarkdownPreview(content, COLLAPSED_LENGTH);

    renderCollapsed(content, "needle");

    expect(screen.queryByText("needle")).toBeNull();
    expect(hasHighlightableMarkdownMatch(preview, "needle")).toBe(false);
    expect(hasHighlightableMarkdownMatch(content, "needle")).toBe(true);
  });

  it("hides a match that crosses the collapse boundary", () => {
    const content = `${"a".repeat(1497)}needle${"b".repeat(1600)}`;
    const preview = createCollapsedMarkdownPreview(content, COLLAPSED_LENGTH);

    expect(preview).not.toContain("needle");
    expect(hasHighlightableMarkdownMatch(preview, "needle")).toBe(false);
  });

  it("reports matches that only exist in a hidden link URL", () => {
    expect(
      hasHighlightableMarkdownMatch(
        "See [docs](https://hidden-domain.example/page).",
        "hidden-domain",
      ),
    ).toBe(false);
  });

  it("reports matches that are visible in rendered Markdown", () => {
    renderMarkdown("The **important result** is ready.", "result");

    expect(screen.getByText("result").tagName).toBe("MARK");
    expect(
      hasHighlightableMarkdownMatch(
        "The **important result** is ready.",
        "result",
      ),
    ).toBe(true);
  });

  it("closes a truncated code fence before rendering the preview ellipsis", () => {
    const { container } = renderCollapsed(
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

  it("keeps a truncated fence inside a blockquote from spawning an extra code block", () => {
    const { container } = renderCollapsed(
      ["> ```ts", `> const value = "${"x".repeat(3200)}";`, "> ```"].join("\n"),
    );
    const codeBlocks = container.querySelectorAll("pre");

    expect(codeBlocks).toHaveLength(1);
    expect(codeBlocks[0]).not.toHaveTextContent("…");
    expect(container).toHaveTextContent("…");
  });

  it("keeps a truncated fence inside a list from spawning an extra code block", () => {
    const { container } = renderCollapsed(
      [
        "- item",
        "",
        "  ```ts",
        `  const value = "${"x".repeat(3200)}";`,
        "  ```",
      ].join("\n"),
    );
    const codeBlocks = container.querySelectorAll("pre");

    expect(codeBlocks).toHaveLength(1);
    expect(codeBlocks[0]).not.toHaveTextContent("…");
    expect(container).toHaveTextContent("…");
  });

  it("keeps a truncated fence inside a quoted list item", () => {
    const { container } = renderCollapsed(
      ["> - ```ts", `>   const value = "${"x".repeat(3200)}";`, ">   ```"].join(
        "\n",
      ),
    );

    expect(container.querySelectorAll("pre")).toHaveLength(1);
    expect(container.querySelector("pre")).not.toHaveTextContent("…");
    expect(container).toHaveTextContent("…");
  });

  it("reports matches that only exist in an HTML entity source", () => {
    expect(hasHighlightableMarkdownMatch("A &amp; B", "amp")).toBe(false);
  });

  it("reports matches that only exist in an escape sequence", () => {
    expect(hasHighlightableMarkdownMatch("A \\* B", "\\*")).toBe(false);
  });

  // 表格行里 cell 之间的分隔符与空白渲染时不输出，只有 cell 内容可见。
  it("reports matches that only exist between table cells", () => {
    expect(
      hasHighlightableMarkdownMatch(
        ["| a | b |", "| --- | --- |", "| 1 | 2 |"].join("\n"),
        " ",
      ),
    ).toBe(false);
  });

  it("reports matches that only exist in unnormalized inline code", () => {
    const { container } = renderMarkdown("Run ` cargo test ` now.");

    expect(container.querySelector("code")?.textContent).toBe("cargo test");
    expect(
      hasHighlightableMarkdownMatch("Run ` cargo test ` now.", " cargo test "),
    ).toBe(false);
  });

  it("resolves collapsed and shortcut reference links", () => {
    renderMarkdown(
      [
        "[docs][] and [guide]",
        "",
        "[docs]: https://example.com/docs",
        "[guide]: https://example.com/guide",
      ].join("\n"),
    );

    expect(screen.getByRole("link", { name: "docs" })).toHaveAttribute(
      "href",
      "https://example.com/docs",
    );
    expect(screen.getByRole("link", { name: "guide" })).toHaveAttribute(
      "href",
      "https://example.com/guide",
    );
  });

  it("resolves reference-style images through their definitions", async () => {
    const user = userEvent.setup();
    renderMarkdown(
      ["![diagram][asset]", "", "[asset]: https://example.com/a.png"].join(
        "\n",
      ),
    );

    await user.click(screen.getByRole("button", { name: /diagram/ }));

    expect(screen.getByRole("img", { name: "diagram" })).toHaveAttribute(
      "src",
      "https://example.com/a.png",
    );
  });

  it("accepts link and image targets wrapped in angle brackets", async () => {
    const user = userEvent.setup();
    renderMarkdown(
      "[docs](<https://example.com/page>)\n\n![diagram](<https://example.com/a.png>)",
    );

    expect(screen.getByRole("link", { name: "docs" })).toHaveAttribute(
      "href",
      "https://example.com/page",
    );
    await user.click(screen.getByRole("button", { name: /diagram/ }));
    expect(screen.getByRole("img", { name: "diagram" })).toHaveAttribute(
      "src",
      "https://example.com/a.png",
    );
  });

  // lezer 对任何 `[...]` 都产出 Link 节点；未定义引用时必须按 CommonMark
  // 原样输出，否则 `[0]`、工具调用摘要 `[Tool: shell]` 会丢掉方括号。
  it("keeps unresolved bracket text literal", () => {
    const { container } = renderMarkdown(
      "index [0] and arr[1] and [Tool: shell] and [TODO]",
    );

    expect(screen.queryByRole("link")).toBeNull();
    expect(container).toHaveTextContent(
      "index [0] and arr[1] and [Tool: shell] and [TODO]",
    );
  });

  it("keeps unresolved reference links and images literal but renders inner formatting", () => {
    const { container } = renderMarkdown(
      "[docs][missing] and ![diagram][missing] and [**bold**]",
    );

    expect(screen.queryByRole("link")).toBeNull();
    expect(screen.queryByRole("button", { name: /加载远程图片/ })).toBeNull();
    expect(container).toHaveTextContent(
      "[docs][missing] and ![diagram][missing] and [bold]",
    );
    expect(screen.getByText("bold").tagName).toBe("STRONG");
  });

  it("keeps links and images with unsupported targets literal", () => {
    const { container } = renderMarkdown(
      [
        "[config.rs](src/config.rs) and ![shot](./shot.png)",
        "[unsafe](javascript:alert(1)) and <ftp://example.com/file>",
      ].join("\n"),
    );

    expect(screen.queryByRole("link")).toBeNull();
    expect(screen.queryByRole("button", { name: /加载远程图片/ })).toBeNull();
    expect(container).toHaveTextContent("[config.rs](src/config.rs)");
    expect(container).toHaveTextContent("![shot](./shot.png)");
    expect(container).toHaveTextContent("[unsafe](javascript:alert(1))");
    expect(container).toHaveTextContent("<ftp://example.com/file>");
  });

  it("highlights matches inside literal link targets", () => {
    const content = "See [config.rs](src/config.rs).";
    renderMarkdown(content, "src/config");

    expect(screen.getByText("src/config").tagName).toBe("MARK");
    expect(hasHighlightableMarkdownMatch(content, "src/config")).toBe(true);
  });

  it("excludes the link title and its surrounding whitespace from the label", () => {
    const { container } = renderMarkdown(
      '[docs](https://example.com "Documentation") next',
    );
    const link = screen.getByRole("link", { name: "docs" });

    expect(link.textContent).toBe("docs");
    expect(link).toHaveAttribute("href", "https://example.com");
    expect(container).toHaveTextContent("docs next");
    expect(container).not.toHaveTextContent("Documentation");
  });

  it("reports matches that only exist in a link title", () => {
    expect(
      hasHighlightableMarkdownMatch(
        '[docs](https://example.com "Documentation")',
        "Documentation",
      ),
    ).toBe(false);
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

  describe("links", () => {
    it("does not navigate and hands the URL to the open callback", () => {
      const onOpenLink = vi.fn();
      render(
        <SessionMarkdown
          content="[docs](https://example.com/a)"
          onOpenLink={onOpenLink}
        />,
      );
      const link = screen.getByRole("link", { name: "docs" });

      expect(link).toHaveAttribute("title", "https://example.com/a");
      expect(link).not.toHaveAttribute("target");

      const notCancelled = fireEvent.click(link);

      expect(notCancelled).toBe(false);
      expect(onOpenLink).toHaveBeenCalledWith("https://example.com/a");
    });

    it("opens links through the open_external command by default", async () => {
      const user = userEvent.setup();
      renderMarkdown("Mail <user@example.com> or visit www.example.com");

      await user.click(screen.getByRole("link", { name: "user@example.com" }));
      await user.click(screen.getByRole("link", { name: "www.example.com" }));

      expect(invokeMock).toHaveBeenCalledWith("open_external", {
        url: "mailto:user@example.com",
      });
      expect(invokeMock).toHaveBeenCalledWith("open_external", {
        url: "https://www.example.com",
      });
    });

    it("renders non-whitelisted protocols as plain text that cannot be clicked", () => {
      const onOpenLink = vi.fn();
      const { container } = render(
        <SessionMarkdown
          content={
            "[a](javascript:alert(1)) [b](data:text/html,hi) [c](vbscript:x)"
          }
          onOpenLink={onOpenLink}
        />,
      );

      expect(container.querySelector("a")).toBeNull();
      expect(container).toHaveTextContent("[a](javascript:alert(1))");
      fireEvent.click(screen.getByText(/javascript:alert/));
      expect(onOpenLink).not.toHaveBeenCalled();
      expect(invokeMock).not.toHaveBeenCalled();
    });
  });

  describe("local paths", () => {
    it("renders file:// links as a path chip instead of a link", async () => {
      const user = userEvent.setup();
      const onRevealPath = vi.fn();
      const { container } = render(
        <SessionMarkdown
          content="See [config](file:///Users/me/proj/src/config.rs)."
          projectDir="/Users/me/proj"
          onRevealPath={onRevealPath}
        />,
      );

      expect(screen.queryByRole("link")).toBeNull();
      const chip = container.querySelector("[data-path-chip]");
      expect(chip).not.toBeNull();
      expect(chip).toHaveTextContent("config");

      const copyButton = screen.getByRole("button", { name: /复制路径/ });
      expect(copyButton).toHaveAttribute(
        "title",
        "/Users/me/proj/src/config.rs",
      );
      await user.click(copyButton);
      expect(copyTextMock).toHaveBeenCalledWith("/Users/me/proj/src/config.rs");

      await user.click(
        screen.getByRole("button", { name: "在 Finder 中显示" }),
      );
      expect(onRevealPath).toHaveBeenCalledWith("/Users/me/proj/src/config.rs");
    });

    it("renders local image paths as a shortened path chip", async () => {
      const user = userEvent.setup();
      const { container } = render(
        <SessionMarkdown
          content="![shot](/Users/me/proj/out/shot.png)"
          projectDir="/Users/me/proj"
        />,
      );

      expect(container.querySelector("img")).toBeNull();
      expect(container.querySelector("[data-path-chip]")).toHaveTextContent(
        "out/shot.png",
      );

      await user.click(
        screen.getByRole("button", { name: "在 Finder 中显示" }),
      );
      expect(invokeMock).toHaveBeenCalledWith("reveal_session_path", {
        path: "/Users/me/proj/out/shot.png",
      });
    });

    it("keeps search consistent with the shortened path shown in the chip", () => {
      const content = "![shot](file:///Users/me/proj/out/shot.png)";

      expect(
        hasHighlightableMarkdownMatch(content, "out/shot", "/Users/me/proj"),
      ).toBe(true);
      expect(
        hasHighlightableMarkdownMatch(content, "/Users/me", "/Users/me/proj"),
      ).toBe(false);
    });
  });

  describe("oversized content", () => {
    const filler = "x".repeat(MARKDOWN_PARSE_LIMIT);

    it("falls back to plain text with fenced code without parsing Markdown", () => {
      const parseSpy = vi.spyOn(markdownLanguage.parser, "parse");

      try {
        const { container } = renderMarkdown(
          `## Heading **bold**\n\n${filler}\n\n\`\`\`ts\nconst big = 1;\n\`\`\``,
          "big",
        );

        expect(parseSpy).not.toHaveBeenCalled();
        expect(container.querySelector("h2")).toBeNull();
        expect(container.querySelector("strong")).toBeNull();
        expect(container).toHaveTextContent("## Heading **bold**");
        expect(screen.getByText("big").tagName).toBe("MARK");
        expect(screen.getByText("big").closest("pre")).not.toBeNull();
      } finally {
        parseSpy.mockRestore();
      }
    });

    it("checks search visibility without parsing oversized content", () => {
      const parseSpy = vi.spyOn(markdownLanguage.parser, "parse");

      try {
        const content = `[docs](https://hidden.example)\n${filler}`;
        expect(hasHighlightableMarkdownMatch(content, "hidden.example")).toBe(
          true,
        );
        expect(parseSpy).not.toHaveBeenCalled();
      } finally {
        parseSpy.mockRestore();
      }
    });
  });
});
