import { describe, expect, it } from "vitest";

import {
  getIconMetadata,
  getIconUrl,
  hasIcon,
  isUrlIcon,
} from "@/icons/extracted";

describe("DevEco Code icon", () => {
  it("is registered as a raster icon, not the generic Huawei mark", () => {
    // The upstream asset is a Windows .ico of raster bitmaps, so it ships as a
    // URL-backed PNG rather than an inline SVG string.
    expect(hasIcon("deveco")).toBe(true);
    expect(isUrlIcon("deveco")).toBe(true);
    expect(getIconUrl("deveco")).toBeTruthy();
  });

  it("is searchable under its own name", () => {
    const metadata = getIconMetadata("deveco");

    expect(metadata?.displayName).toBe("DevEco Code");
    expect(metadata?.keywords).toContain("harmonyos");
  });
});
