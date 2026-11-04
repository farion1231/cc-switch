import fs from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";

const css = fs.readFileSync(
  path.resolve(__dirname, "../../src/index.css"),
  "utf8",
);
const darkTokens = [...css.matchAll(/\.dark\s*\{([^}]+)\}/g)]
  .map((match) => match[1])
  .join("\n");

function token(name: string): string {
  const value = darkTokens.match(new RegExp(`--${name}:\\s*([^;]+);`))?.[1];
  if (!value) throw new Error(`Missing dark token: ${name}`);
  return value.trim();
}

function luminance(hex: string): number {
  const channels = [1, 3, 5].map((offset) => {
    const value = parseInt(hex.slice(offset, offset + 2), 16) / 255;
    return value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4;
  });
  return channels[0] * 0.2126 + channels[1] * 0.7152 + channels[2] * 0.0722;
}

describe("provider item theme alignment", () => {
  it("uses the combined implementation's dark text hierarchy", () => {
    expect([token("text-1"), token("text-2"), token("text-3")]).toEqual([
      "#e4e4e7",
      "#ababb3",
      "#9a9aa2",
    ]);
  });

  it("keeps inherited foreground colors aligned with provider text", () => {
    for (const name of [
      "foreground",
      "card-foreground",
      "popover-foreground",
      "secondary-foreground",
      "accent-foreground",
    ]) {
      expect(token(name)).toBe("240 5.9% 90%");
    }
  });

  it("keeps secondary and muted text readable on card and selected surfaces", () => {
    for (const text of [token("text-2"), token("text-3")]) {
      for (const background of [token("bg-card"), token("bg-selected")]) {
        const ratio = (luminance(text) + 0.05) / (luminance(background) + 0.05);
        expect(ratio).toBeGreaterThanOrEqual(4.5);
      }
    }
  });
});
