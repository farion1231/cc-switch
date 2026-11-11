import { describe, expect, it } from "vitest";

import { getIconUrl, hasIcon, isUrlIcon } from "@/icons/extracted";
import { getIconMetadata } from "@/icons/extracted/metadata";

describe("Cursor icon", () => {
  it("registers cursor in the extracted icon registry", () => {
    expect(hasIcon("cursor")).toBe(true);
    expect(isUrlIcon("cursor")).toBe(true);
    expect(getIconUrl("cursor")).toBeTruthy();

    const meta = getIconMetadata("cursor");
    expect(meta).toBeDefined();
    expect(meta?.displayName).toBe("Cursor");
  });
});
