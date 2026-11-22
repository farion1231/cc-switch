import { render } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import {
  countMatches,
  createHighlightedSnippet,
  findTextMatches,
  highlightText,
  highlightTextRanges,
} from "@/components/sessions/utils";
import type { SessionTextRange } from "@/types";
import { getAgentReaderStyle } from "@/components/sessions/reader/agentStyles";
import { buildTurns, findSearchHits } from "@/components/sessions/reader/turns";
import { msg, text } from "./sessions/reader/helpers";

const cases: [string, string, string[]][] = [
  ["İstanbul", "i", ["İ"]],
  ["İstanbul", "i\u0307", ["İ"]],
  ["İstanbul", "\u0307", ["İ"]],
  ["İstanbul", "istanbul", []],
  ["İİ before TARGET after", "target", ["TARGET"]],
  ["ΟΣ ΟΣΑ ος οσ", "ΟΣ", ["ΟΣ", "ος"]],
  ["ΟΣ ΟΣΑ ος οσ", "σ", ["Σ", "σ"]],
  ["ΟΣ ΟΣΑ ος οσ", "ς", ["Σ", "ς"]],
  ["ſ s S", "s", ["s", "S"]],
  ["ſ s S", "ſ", ["ſ"]],
  ["K K k", "k", ["K", "K", "k"]],
  ["𐐀𐐨", "𐐨", ["𐐀", "𐐨"]],
  ["İ😀中文😀", "😀", ["😀", "😀"]],
  ["İ😀中文测试中文", "中文", ["中文", "中文"]],
  ["100%_ [a.*] 100%_", "%_", ["%_", "%_"]],
  ["literal [a.*] stays literal", "[a.*]", ["[a.*]"]],
  ["aaaaa", "aa", ["aa", "aa"]],
  [" A a ", " a ", ["A", "a"]],
  [" A a ", "  ", []],
];

describe("whole-string lowercase session search", () => {
  it("does not duplicate source characters when expanded lowercase matches overlap", () => {
    const source = "İİİ";
    const query = "\u0307i";
    const { container } = render(<div>{highlightText(source, query)}</div>);

    expect(countMatches(source, query)).toBe(2);
    expect(container.textContent).toBe(source);
    expect(
      Array.from(
        container.querySelectorAll("mark"),
        (mark) => mark.textContent,
      ),
    ).toEqual([source]);
  });

  it.each(cases)(
    "keeps counts, turn hits and highlights aligned for %j / %j",
    (source, query, expected) => {
      const turns = buildTurns(
        [msg("user", [text(source)], { turnId: "t1" })],
        {
          style: getAgentReaderStyle("claude"),
        },
      );
      const { container } = render(<div>{highlightText(source, query)}</div>);

      expect(countMatches(source, query)).toBe(expected.length);
      expect(findSearchHits(turns, query)?.total ?? 0).toBe(expected.length);
      expect(
        Array.from(
          container.querySelectorAll("mark"),
          (mark) => mark.textContent,
        ),
      ).toEqual(expected);
      expect(container.textContent).toBe(source);
    },
  );
});

describe("source-derived snippet highlight ranges", () => {
  it("retains contextual sigma matches when the cased prefix is cropped out", () => {
    const source = "A" + "\u0301".repeat(100) + "Σ";
    const snippet = createHighlightedSnippet(
      source,
      [...findTextMatches(source, "ς")],
      41,
      source.length,
    );
    const { container } = render(
      <div>{highlightTextRanges(snippet.text, snippet.highlights)}</div>,
    );

    expect(snippet.text).toBe("…" + "\u0301".repeat(60) + "Σ");
    expect(snippet.highlights).toEqual([{ start: 61, end: 62 }]);
    expect(container.querySelector("mark")).toHaveTextContent("Σ");
  });

  it.each([
    [
      "i\u0307",
      [
        { start: 4, end: 5 },
        { start: 6, end: 7 },
      ],
      ["İ", "İ"],
    ],
    ["İ\n\nİ", [{ start: 4, end: 7 }], ["İ İ"]],
  ] as const)(
    "maps multiple matches and compressed whitespace after emoji: %s",
    (query, expectedRanges, expectedMarks) => {
      const source = "  😀 \t İ\n\nİ <tag>  ";
      const snippet = createHighlightedSnippet(source, [
        ...findTextMatches(source, query),
      ]);
      const { container } = render(
        <div>{highlightTextRanges(snippet.text, snippet.highlights)}</div>,
      );

      expect(snippet.text).toBe(" 😀 İ İ <tag> ");
      expect(snippet.highlights).toEqual(expectedRanges);
      expect(
        Array.from(
          container.querySelectorAll("mark"),
          (mark) => mark.textContent,
        ),
      ).toEqual(expectedMarks);
      expect(container.textContent).toBe(snippet.text);
      expect(container.querySelector("tag")).toBeNull();
    },
  );

  it("skips malformed ranges and surrogate-splitting boundaries without interpreting HTML", () => {
    const source = "<img src=x onerror=alert(1)>😀OK";
    const at = source.indexOf("😀");
    const ranges = [
      null,
      {},
      { start: -1, end: 2 },
      { start: 2, end: 1 },
      { start: 0, end: source.length + 1 },
      { start: 0.5, end: 2 },
      { start: NaN, end: 2 },
      { start: 0, end: Infinity },
      { start: at + 1, end: at + 2 },
      { start: at, end: at + 1 },
      { start: at, end: at + 2 },
    ] as unknown as SessionTextRange[];
    const { container } = render(
      <div>{highlightTextRanges(source, ranges)}</div>,
    );

    expect(container.textContent).toBe(source);
    expect(
      Array.from(
        container.querySelectorAll("mark"),
        (mark) => mark.textContent,
      ),
    ).toEqual(["😀"]);
    expect(container.querySelector("img")).toBeNull();
  });

  it("sorts and merges overlapping ranges without mutating supplied data", () => {
    const ranges = [
      { start: 4, end: 6 },
      { start: 1, end: 3 },
      { start: 2, end: 5 },
    ];
    const before = structuredClone(ranges);
    const { container } = render(
      <div>{highlightTextRanges("abcdef", ranges)}</div>,
    );

    expect(container.textContent).toBe("abcdef");
    expect(container.querySelectorAll("mark")).toHaveLength(1);
    expect(container.querySelector("mark")).toHaveTextContent("bcdef");
    expect(ranges).toEqual(before);
  });

  it.each([undefined, null, {}, "invalid"])(
    "renders unchanged text for a malformed range collection: %s",
    (ranges) => {
      const { container } = render(
        <div>
          {highlightTextRanges(
            "plain text",
            ranges as unknown as SessionTextRange[],
          )}
        </div>,
      );
      expect(container.textContent).toBe("plain text");
      expect(container.querySelector("mark")).toBeNull();
    },
  );

  it("does not lowercase/map each code point in a large ASCII match", () => {
    const source = "A".repeat(1_000_000) + "NEEDLE";
    const lower = vi.spyOn(String.prototype, "toLowerCase");
    try {
      expect([...findTextMatches(source, "needle")]).toEqual([
        { start: 1_000_000, end: 1_000_006 },
      ]);
      expect(lower).toHaveBeenCalledTimes(2);
    } finally {
      lower.mockRestore();
    }
  });
});
