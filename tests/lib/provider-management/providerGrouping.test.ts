import { describe, expect, it } from "vitest";
import type { Provider } from "@/types";
import { buildProviderGroups } from "@/lib/provider-management/providerGrouping";

const makeProvider = (
  id: string,
  overrides: Partial<Provider> = {},
): Provider => ({
  id,
  name: overrides.name ?? id,
  settingsConfig: overrides.settingsConfig ?? {},
  category: overrides.category ?? "aggregator",
  meta: overrides.meta,
});

describe("providerGrouping", () => {
  it("groups providers by explicit providerGroup metadata", () => {
    const groups = buildProviderGroups(
      [
        makeProvider("minimax-a", {
          meta: { providerGroup: "Minimax" },
        }),
        makeProvider("minimax-b", {
          meta: { providerGroup: "Minimax" },
        }),
      ],
      "claude",
    );

    expect(groups).toHaveLength(1);
    expect(groups[0].id).toBe("group:minimax");
    expect(groups[0].label).toBe("Minimax");
    expect(groups[0].providers.map((provider) => provider.id)).toEqual([
      "minimax-a",
      "minimax-b",
    ]);
  });

  it("falls back to grouping aggregator providers by base URL host", () => {
    const groups = buildProviderGroups(
      [
        makeProvider("a", {
          settingsConfig: {
            env: { ANTHROPIC_BASE_URL: "https://api.example.com/v1" },
          },
        }),
        makeProvider("b", {
          settingsConfig: {
            env: { ANTHROPIC_BASE_URL: "https://api.example.com/anthropic" },
          },
        }),
        makeProvider("c", {
          settingsConfig: {
            env: { ANTHROPIC_BASE_URL: "https://other.example.com/v1" },
          },
        }),
      ],
      "claude",
    );

    expect(groups).toHaveLength(2);
    expect(groups[0].label).toBe("api.example.com");
    expect(groups[0].providers.map((provider) => provider.id)).toEqual([
      "a",
      "b",
    ]);
    expect(groups[1].providers.map((provider) => provider.id)).toEqual(["c"]);
  });
});
