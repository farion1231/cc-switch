import { describe, expect, it } from "vitest";

import { getIcon, hasIcon } from "@/icons/extracted";

describe("Pi icon", () => {
  it("uses the official README mark", () => {
    const icon = getIcon("pi");

    expect(hasIcon("pi")).toBe(true);
    expect(icon).toContain('viewBox="0 0 800 800"');
    expect(icon).toContain(
      "M165.29 165.29H517.36V400H400V517.36H282.65V634.72H165.29Z",
    );
    expect(icon).toContain("M517.36 400H634.72V634.72H517.36Z");
  });
});
