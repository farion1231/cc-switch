import { describe, expect, it } from "vitest";
import { isWithinVerticalViewport } from "../../e2e/viewportGeometry";

describe("Task 7 vertical viewport geometry", () => {
  it("rejects an expected element entirely below the viewport", () => {
    expect(isWithinVerticalViewport({ top: 900, bottom: 940 }, 844)).toBe(
      false,
    );
  });
});
