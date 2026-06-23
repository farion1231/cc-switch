import fs from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";

const PROVIDER_CARD_TSX = path.resolve(
  __dirname,
  "..",
  "..",
  "src",
  "components",
  "providers",
  "ProviderCard.tsx",
);
const INDEX_CSS = path.resolve(__dirname, "..", "..", "src", "index.css");

describe("ProviderCard layout", () => {
  const source = fs.readFileSync(PROVIDER_CARD_TSX, "utf8");
  const indexCss = fs.readFileSync(INDEX_CSS, "utf8");

  it("lets website links use available card width before truncating", () => {
    expect(source).not.toContain("max-w-[280px]");
    expect(source).toContain("flex min-w-0 flex-1 items-center gap-2");
    expect(source).toContain("min-w-0 flex-1 space-y-1");
    expect(source).toContain(
      "inline-flex max-w-full items-center overflow-hidden text-left text-sm",
    );
  });

  it("uses outlined grab cursors for provider drag sorting", () => {
    expect(source).toContain("provider-card-grab-cursor");
    expect(source).toContain("provider-card-grabbing-cursor");
    expect(indexCss).toContain(".provider-card-grab-cursor");
    expect(indexCss).toContain(".provider-card-grabbing-cursor");
    expect(indexCss).toContain("stroke='%2318171f'");
    expect(indexCss).toMatch(/,\s*grab/);
    expect(indexCss).toMatch(/,\s*grabbing/);
  });
});
