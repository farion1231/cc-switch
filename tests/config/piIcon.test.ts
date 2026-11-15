import { describe, expect, it } from "vitest";

import { getIcon, hasIcon } from "@/icons/extracted";

describe("Pi icon", () => {
  it("uses the official three-color pi.dev mark", () => {
    const icon = getIcon("pi");

    expect(hasIcon("pi")).toBe(true);
    expect(icon).toContain('viewBox="0 0 800 800"');
    expect(icon).toContain(
      'fill="#F09082" d="M165.29 165.29H517.36V400H400V282.65H165.29Z"',
    );
    expect(icon).toContain(
      'fill="#4D9ABF" d="M165.29 282.65H282.65V400H400V517.36H282.65V634.72H165.29Z"',
    );
    expect(icon).toContain(
      'fill="#F1BE58" d="M517.36 400H634.72V634.72H517.36Z"',
    );
    expect(icon).not.toContain("currentColor");
  });
});
