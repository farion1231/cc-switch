import { describe, expect, it } from "vitest";
import type { Provider } from "@/types";
import {
  buildProviderGroupSortUpdates,
  buildProviderGroups,
} from "@/lib/provider-management/providerGrouping";

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

  it("groups duplicate branded providers by display name", () => {
    const groups = buildProviderGroups(
      [
        makeProvider("kimi-a", {
          name: "Kimi For Coding",
          category: "cn_official",
          settingsConfig: {
            env: { ANTHROPIC_BASE_URL: "https://api-a.example.com/coding" },
          },
        }),
        makeProvider("kimi-b", {
          name: "Kimi For Coding",
          category: "cn_official",
          settingsConfig: {
            env: { ANTHROPIC_BASE_URL: "https://api-b.example.com/coding" },
          },
        }),
      ],
      "claude",
    );

    expect(groups).toHaveLength(1);
    expect(groups[0].id).toBe("name:kimi-for-coding");
    expect(groups[0].label).toBe("Kimi For Coding");
    expect(groups[0].providers.map((provider) => provider.id)).toEqual([
      "kimi-a",
      "kimi-b",
    ]);
  });

  it("moves every provider in a grouped drawer when sorting groups", () => {
    const groups = buildProviderGroups(
      [
        makeProvider("kimi-a", {
          name: "Kimi For Coding",
          category: "cn_official",
        }),
        makeProvider("kimi-b", {
          name: "Kimi For Coding",
          category: "cn_official",
        }),
        makeProvider("atlas", {
          name: "AtlasCloud",
          category: "cn_official",
        }),
      ],
      "claude",
    );

    const updates = buildProviderGroupSortUpdates(
      groups,
      "name:kimi-for-coding",
      "atlas",
    );

    expect(updates).toEqual([
      { id: "atlas", sortIndex: 0 },
      { id: "kimi-a", sortIndex: 1 },
      { id: "kimi-b", sortIndex: 2 },
    ]);
  });

  it("keeps hidden providers in the global sort update when sorting filtered groups", () => {
    const allGroups = buildProviderGroups(
      [
        makeProvider("atlas", {
          name: "AtlasCloud",
          category: "cn_official",
        }),
        makeProvider("kimi-a", {
          name: "Kimi For Coding",
          category: "cn_official",
        }),
        makeProvider("kimi-b", {
          name: "Kimi For Coding",
          category: "cn_official",
        }),
        makeProvider("minimax", {
          name: "MiniMax",
          category: "cn_official",
        }),
      ],
      "claude",
    );
    const visibleGroups = allGroups.filter(
      (group) => group.id === "atlas" || group.id === "minimax",
    );

    const updates = buildProviderGroupSortUpdates(
      allGroups,
      "atlas",
      "minimax",
      visibleGroups,
    );

    expect(updates).toEqual([
      { id: "minimax", sortIndex: 0 },
      { id: "kimi-a", sortIndex: 1 },
      { id: "kimi-b", sortIndex: 2 },
      { id: "atlas", sortIndex: 3 },
    ]);
  });

  it("maps filtered single-provider rows back to their full drawer group when sorting", () => {
    const allProviders = [
      makeProvider("atlas", {
        name: "AtlasCloud",
        category: "cn_official",
      }),
      makeProvider("kimi-a", {
        name: "Kimi For Coding",
        category: "cn_official",
      }),
      makeProvider("kimi-b", {
        name: "Kimi For Coding",
        category: "cn_official",
      }),
      makeProvider("minimax", {
        name: "MiniMax",
        category: "cn_official",
      }),
    ];
    const allGroups = buildProviderGroups(allProviders, "claude");
    const visibleGroups = buildProviderGroups(
      allProviders.filter((provider) =>
        ["atlas", "kimi-a"].includes(provider.id),
      ),
      "claude",
    );

    const updates = buildProviderGroupSortUpdates(
      allGroups,
      "kimi-a",
      "atlas",
      visibleGroups,
    );

    expect(updates).toEqual([
      { id: "kimi-a", sortIndex: 0 },
      { id: "kimi-b", sortIndex: 1 },
      { id: "atlas", sortIndex: 2 },
      { id: "minimax", sortIndex: 3 },
    ]);
  });
});
