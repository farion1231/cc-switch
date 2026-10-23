import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { ProviderIcon, scopeSvgIds } from "@/components/ProviderIcon";

describe("scopeSvgIds", () => {
  it("suffixes ids and every reference to them", () => {
    const svg =
      '<svg><path fill="url(#g)" clip-path="url(#c)"/><use href="#g"/><defs><linearGradient id="g"/><clipPath id="c"/></defs><rect data-id="keep"/></svg>';
    expect(scopeSvgIds(svg, "s1")).toBe(
      '<svg><path fill="url(#g-s1)" clip-path="url(#c-s1)"/><use href="#g-s1"/><defs><linearGradient id="g-s1"/><clipPath id="c-s1"/></defs><rect data-id="keep"/></svg>',
    );
  });

  it("returns SVGs without ids unchanged", () => {
    const svg = '<svg><path fill="currentColor"/></svg>';
    expect(scopeSvgIds(svg, "s1")).toBe(svg);
  });
});

describe("ProviderIcon inline SVG ids", () => {
  it("gives each rendered copy of a gradient icon its own ids", () => {
    const { container } = render(
      <>
        <ProviderIcon icon="qwen" name="" />
        <ProviderIcon icon="qwen" name="" />
      </>,
    );
    const gradients = [...container.querySelectorAll("linearGradient")];
    expect(gradients).toHaveLength(2);
    const [first, second] = gradients.map((node) => node.id);
    expect(first).not.toBe(second);
    // Each copy's path points at its own gradient, never at the other copy.
    const fills = [...container.querySelectorAll("path[fill^='url(']")].map(
      (node) => node.getAttribute("fill"),
    );
    expect(fills).toEqual([`url(#${first})`, `url(#${second})`]);
  });
});
