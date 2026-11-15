import { describe, expect, it } from "vitest";
import { aimingAt } from "@/tray-panel/menuAim";

// 子面板在主面板右边：近边 x=300，纵向 100–400
const right = { top: 100, bottom: 400, left: 300, right: 600 };
// 子面板在主面板左边：近边 x=300（子面板的右边）
const left = { top: 100, bottom: 400, left: 0, right: 300 };

describe("aimingAt", () => {
  it("counts a diagonal move toward the sub panel", () => {
    expect(
      aimingAt({ x: 200, y: 200 }, { x: 210, y: 205 }, right, "right"),
    ).toBe(true);
    expect(aimingAt({ x: 400, y: 200 }, { x: 390, y: 205 }, left, "left")).toBe(
      true,
    );
  });

  it("switches right away on a vertical move between app rows", () => {
    expect(
      aimingAt({ x: 200, y: 200 }, { x: 200, y: 230 }, right, "right"),
    ).toBe(false);
  });

  it("ignores moves away from the sub panel and a still pointer", () => {
    expect(
      aimingAt({ x: 200, y: 200 }, { x: 190, y: 200 }, right, "right"),
    ).toBe(false);
    expect(
      aimingAt({ x: 200, y: 200 }, { x: 200, y: 200 }, right, "right"),
    ).toBe(false);
  });

  it("ignores a pointer already past the sub panel's near edge", () => {
    expect(
      aimingAt({ x: 310, y: 200 }, { x: 320, y: 200 }, right, "right"),
    ).toBe(false);
  });
});
