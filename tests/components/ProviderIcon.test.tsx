import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { getProviderInitials, ProviderIcon } from "@/components/ProviderIcon";

describe("getProviderInitials", () => {
  it("extracts uppercase initials from single or multiple words", () => {
    expect(getProviderInitials("Anthropic")).toBe("A");
    expect(getProviderInitials("OpenAI")).toBe("O");
    expect(getProviderInitials("Google Gemini")).toBe("GG");
    expect(getProviderInitials("DeepSeek V3")).toBe("DV");
  });

  it("safely ignores leading, trailing, and redundant consecutive whitespace", () => {
    expect(getProviderInitials("  Custom   Model   Provider  ")).toBe("CM");
    expect(getProviderInitials("   ")).toBe("");
    expect(getProviderInitials("")).toBe("");
  });

  it("handles Unicode surrogate pairs and emojis without truncating code units", () => {
    // Emojis like 🤖 (\ud83e\udd16) and 🚀 (\ud83d\ude80) have 2 UTF-16 code units.
    // Naive string indexing (word[0]) cuts the surrogate pair in half, leaving an unpaired surrogate (\ud83e)
    // that crashes encodeURIComponent and renders corrupted glyphs.
    const emojiFirst = getProviderInitials("🤖 AI");
    expect(emojiFirst).toBe("🤖A");
    expect(() => encodeURIComponent(emojiFirst)).not.toThrow();

    // Naive .slice(0, 2) on "A🚀" (length 3 in UTF-16) cuts the emoji in half ("A\ud83d").
    const emojiSecond = getProviderInitials("Alpha 🚀");
    expect(emojiSecond).toBe("A🚀");
    expect(() => encodeURIComponent(emojiSecond)).not.toThrow();

    const dualEmoji = getProviderInitials("🚀 🔥");
    expect(dualEmoji).toBe("🚀🔥");
    expect(() => encodeURIComponent(dualEmoji)).not.toThrow();

    const singleEmoji = getProviderInitials("🚀");
    expect(singleEmoji).toBe("🚀");
  });
});

describe("ProviderIcon component fallback", () => {
  it("renders safe initials in the fallback container when no icon matches", () => {
    render(<ProviderIcon name="🤖 AI Assistant" icon="non-existent-icon" />);
    expect(screen.getByText("🤖A")).toBeInTheDocument();
  });

  it("renders safe initials when name contains extra whitespace", () => {
    render(<ProviderIcon name="  DeepSeek   V3  " />);
    expect(screen.getByText("DV")).toBeInTheDocument();
  });

  it("renders nothing when showFallback is false and icon is missing", () => {
    const { container } = render(
      <ProviderIcon name="Unknown" showFallback={false} />,
    );
    expect(container.firstChild).toBeNull();
  });
});
