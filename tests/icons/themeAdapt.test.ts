import { describe, expect, it } from "vitest";
import {
  adaptSvgToTheme,
  getIcon,
  isDarkInk,
  needsLightBacking,
} from "@/icons/extracted";

describe("isDarkInk", () => {
  it("treats only near-black as dark, not saturated dark colors", () => {
    expect(isDarkInk("#000000")).toBe(true);
    expect(isDarkInk("#111")).toBe(true);
    expect(isDarkInk("#3A3B45")).toBe(true);
    expect(isDarkInk("#C7000B")).toBe(false);
    expect(isDarkInk("#FF0000")).toBe(false);
    expect(isDarkInk("#FFD21E")).toBe(false);
  });
});

describe("adaptSvgToTheme", () => {
  it("keeps near-black details on multi-color icons", () => {
    const hf = getIcon("huggingface");
    expect(adaptSvgToTheme(hf)).toBe(hf);
    const lion = getIcon("lioncc");
    expect(adaptSvgToTheme(lion)).toBe(lion);
  });

  it("recolors single-ink near-black icons", () => {
    const svg = '<svg><path fill="#000000"/><path stroke="#111"/></svg>';
    expect(adaptSvgToTheme(svg)).toBe(
      '<svg><path fill="currentColor"/><path stroke="currentColor"/></svg>',
    );
  });

  it("leaves saturated reds alone", () => {
    expect(adaptSvgToTheme(getIcon("huawei"))).toContain("#C7000B");
  });

  it("ignores paints inside masks and clip paths", () => {
    const svg =
      '<svg><defs><clipPath id="a"><rect fill="white"/></clipPath></defs><path fill="#000000" d="M0 0"/></svg>';
    const out = adaptSvgToTheme(svg);
    expect(out).toContain('<rect fill="white"/>');
    expect(out).toContain('<path fill="currentColor"');
  });

  it("leaves icons that also paint near-white untouched", () => {
    const svg = '<svg><path fill="#111111"/><path fill="#ffffff"/></svg>';
    expect(adaptSvgToTheme(svg)).toBe(svg);
  });
});

describe("needsLightBacking", () => {
  it("is false for tile and themable icons, true for raster icons", () => {
    expect(needsLightBacking("sssaicode")).toBe(false);
    expect(needsLightBacking("huggingface")).toBe(false);
    expect(needsLightBacking("openai")).toBe(false);
    expect(needsLightBacking("atlascloud")).toBe(true);
  });
});
