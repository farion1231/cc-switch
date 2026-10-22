import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { ProviderIconBox } from "@/components/ProviderIconBox";
import { hasIcon, isTileIcon } from "@/icons/extracted";
import { iconMetadata } from "@/icons/extracted/metadata";

describe("ProviderIconBox", () => {
  it("lets icons with their own solid background fill the box", () => {
    const { container } = render(<ProviderIconBox icon="88api" name="88API" />);
    const box = container.firstElementChild as HTMLElement;
    const img = box.querySelector("img") as HTMLImageElement;

    expect(box.className).not.toContain("border-border");
    expect(img.style.width).toBe("32px");
    expect(img.className).toContain("object-cover");
    expect(img.className).not.toContain("object-contain");
  });

  it("keeps transparent logos small and centered inside a bordered box", () => {
    const { container } = render(
      <ProviderIconBox icon="dmxapi" name="DMXAPI" />,
    );
    const box = container.firstElementChild as HTMLElement;
    const img = box.querySelector("img") as HTMLImageElement;

    expect(box.className).toContain("border-border");
    expect(img.style.width).toBe("22px");
    expect(img.className).toContain("object-contain");
  });

  it("only marks icons that exist in the icon index", () => {
    const tiles = Object.keys(iconMetadata).filter(isTileIcon);

    expect(tiles.length).toBeGreaterThan(0);
    for (const name of tiles) {
      expect(hasIcon(name), name).toBe(true);
    }
  });
});
