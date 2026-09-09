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
const PROVIDER_LIST_TSX = path.resolve(
  __dirname,
  "..",
  "..",
  "src",
  "components",
  "providers",
  "ProviderList.tsx",
);

describe("ProviderCard layout", () => {
  const source = fs.readFileSync(PROVIDER_CARD_TSX, "utf8");

  it("lets website links use available card width before truncating", () => {
    expect(source).not.toContain("max-w-[280px]");
    expect(source).toContain("flex min-w-0 flex-1 items-center gap-2");
    expect(source).toContain("min-w-0 flex-1 space-y-1");
    expect(source).toContain(
      "inline-flex max-w-full items-center overflow-hidden text-left text-sm",
    );
  });

  it("公网路由 badge is clickable and opens 公网路由 settings via callback", () => {
    expect(source).toContain("onOpenPublicRouteSettings");
    expect(source).toMatch(
      /onClick=\{[\s\S]*?onOpenPublicRouteSettings[\s\S]*?\}[\s\S]*?cursor\.needsPublicRoute/,
    );
    expect(source).toMatch(/type="button"[\s\S]*?cursor\.needsPublicRoute|cursor\.needsPublicRoute[\s\S]*?type="button"/);
  });

  it("does not add a Pi-only model list to provider cards", () => {
    expect(source).not.toContain("ProviderModelSummary");
    expect(source).not.toContain("extractPiModelSummaryItems");
  });

  it("keeps Pi provider cards independent from routing capability state", () => {
    const listSource = fs.readFileSync(PROVIDER_LIST_TSX, "utf8");

    expect(source).not.toContain("piCurrentRoute");
    expect(source).not.toContain("isFailoverEligible");
    expect(listSource).not.toContain("gatewayStatus");
    expect(listSource).not.toContain("activeRoute");
    expect(listSource).not.toContain("pi.gatewayReason");
    expect(listSource).not.toContain("pi.route");
  });
});
