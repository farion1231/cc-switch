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
    expect(source).toContain('<div className="min-w-0 flex-1">');
    expect(source).toContain(
      "inline-flex max-w-full items-center overflow-hidden text-start",
    );
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

  it("keeps per-app managed-auth useQuery hooks out of the shared ProviderCard shell", () => {
    // Per-app auth queries belong inside conditionally mounted subcomponents
    // using `useManagedAuthStatus` so unrelated cards never subscribe to or
    // leak cached data from disabled React Query observers (cf. #9945 / #9975).
    expect(source).not.toMatch(/\buseQuery\s*\(/);
  });
});
