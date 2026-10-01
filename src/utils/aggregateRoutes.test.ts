import { describe, expect, it } from "vitest";
import {
  assignSlotIds,
  canSaveAggregateRoutes,
  flattenProviderGroups,
  groupSlotsByProvider,
  slotId,
  slotLabel,
  TIER_ROW_ORDER,
} from "./aggregateRoutes";

const TIERS = ["fable", "opus", "sonnet", "haiku"] as const;

describe("slotId", () => {
  it("is built from the tier and the ordinal alone", () => {
    expect(slotId("sonnet", 1)).toBe("claude-sonnet-1");
    expect(slotId("fable", 2)).toBe("claude-fable-2");
  });

  it("never embeds a vendor name (a vendor word empties the whole picker)", () => {
    // Claude Desktop drops any model whose id contains a vendor word
    // (deepseek, glm, kimi, gpt, qwen, gemini, …) from the model list — and it
    // drops the *entire* list, not just that entry. The earlier
    // `claude-{tier}-{provider-slug}` scheme hit that blacklist
    // (claude-fable-deepseek, claude-fable-zhipu-glm) and left the picker
    // empty. Ids must stay independent of the provider name; readability
    // belongs to the display name instead.
    for (const tier of TIERS) {
      for (let ordinal = 1; ordinal <= 20; ordinal += 1) {
        expect(slotId(tier, ordinal)).toBe(`claude-${tier}-${ordinal}`);
        expect(slotId(tier, ordinal)).not.toMatch(
          /deepseek|glm|kimi|gpt|gemini|qwen/i,
        );
      }
    }
  });

  it("matches the backend is_claude_safe_model_id shape", () => {
    // claude-{sonnet|opus|haiku|fable}-{non-empty}
    expect(slotId("haiku", 3)).toMatch(/^claude-haiku-.+$/);
    for (const tier of TIERS) {
      expect(slotId(tier, 1)).toMatch(/^claude-(sonnet|opus|haiku|fable)-.+$/);
    }
  });

  it("gives every slot of a tier a distinct id", () => {
    for (const tier of TIERS) {
      const ids = Array.from({ length: 20 }, (_, i) => slotId(tier, i + 1));
      expect(new Set(ids).size).toBe(ids.length);
    }
  });
});

describe("slotLabel", () => {
  it("prefers an explicit label (trimmed)", () => {
    expect(
      slotLabel({ upstreamModel: "glm-5.3", label: "  Zhipu GLM  " }),
    ).toBe("Zhipu GLM");
  });

  it("falls back to the upstream model when no label is set", () => {
    expect(slotLabel({ upstreamModel: "glm-5.3" })).toBe("glm-5.3");
    expect(slotLabel({ upstreamModel: "glm-5.3", label: "   " })).toBe(
      "glm-5.3",
    );
  });
});

describe("canSaveAggregateRoutes", () => {
  const slot = {
    routeId: "claude-sonnet-glm",
    tier: "sonnet" as const,
    providerId: "p-glm",
    upstreamModel: "glm-5.3",
  };

  it("needs at least one slot and a non-empty default target", () => {
    expect(
      canSaveAggregateRoutes({
        slots: [slot],
        defaultTarget: { kind: "providerId", value: "p-glm" },
      }),
    ).toBe(true);
  });

  it("cannot be saved while the default target is blank (nothing picked)", () => {
    expect(
      canSaveAggregateRoutes({
        slots: [slot],
        defaultTarget: { kind: "providerId", value: "" },
      }),
    ).toBe(false);
    expect(
      canSaveAggregateRoutes({
        slots: [slot],
        defaultTarget: { kind: "slotId", value: "   " },
      }),
    ).toBe(false);
  });

  it("cannot be saved with no slots or no table at all", () => {
    expect(
      canSaveAggregateRoutes({
        slots: [],
        defaultTarget: { kind: "providerId", value: "p-glm" },
      }),
    ).toBe(false);
    expect(canSaveAggregateRoutes(undefined)).toBe(false);
    expect(canSaveAggregateRoutes(null)).toBe(false);
  });
});

describe("assignSlotIds", () => {
  const base = {
    defaultTarget: { kind: "providerId" as const, value: "p-glm" },
  };

  it("numbers each tier separately: the 1st and 2nd slot of a tier get -1 and -2", () => {
    const next = assignSlotIds({
      ...base,
      slots: [
        {
          routeId: "",
          tier: "sonnet",
          providerId: "p-ds",
          upstreamModel: "flash",
        },
        {
          routeId: "",
          tier: "sonnet",
          providerId: "p-glm",
          upstreamModel: "pro",
        },
      ],
    });
    expect(next.slots.map((s) => s.routeId)).toEqual([
      "claude-sonnet-1",
      "claude-sonnet-2",
    ]);
  });

  it("starts each tier at 1", () => {
    const next = assignSlotIds({
      ...base,
      slots: [
        { routeId: "", tier: "sonnet", providerId: "p-ds", upstreamModel: "a" },
        { routeId: "", tier: "opus", providerId: "p-ds", upstreamModel: "b" },
        {
          routeId: "",
          tier: "sonnet",
          providerId: "p-glm",
          upstreamModel: "c",
        },
      ],
    });
    expect(next.slots.map((s) => s.routeId)).toEqual([
      "claude-sonnet-1",
      "claude-opus-1",
      "claude-sonnet-2",
    ]);
  });

  it("changes the id when the tier changes", () => {
    const next = assignSlotIds({
      ...base,
      slots: [
        {
          routeId: "claude-sonnet-1",
          tier: "opus",
          providerId: "p-ds",
          upstreamModel: "flash",
        },
      ],
    });
    expect(next.slots[0].routeId).toBe("claude-opus-1");
  });

  it("recomputes every slot, leaving no stale routeId behind", () => {
    const next = assignSlotIds({
      ...base,
      slots: [
        {
          routeId: "claude-sonnet-stale",
          tier: "opus",
          providerId: "p-ds",
          upstreamModel: "flash",
        },
        {
          routeId: "claude-sonnet-stale",
          tier: "sonnet",
          providerId: "p-ds",
          upstreamModel: "pro",
        },
      ],
    });
    expect(next.slots.map((s) => s.routeId)).toEqual([
      "claude-opus-1",
      "claude-sonnet-1",
    ]);
  });

  it("migrates legacy ids that embedded the provider name", () => {
    const next = assignSlotIds({
      ...base,
      slots: [
        {
          routeId: "claude-fable-zhipu-glm",
          tier: "fable",
          providerId: "p-glm",
          upstreamModel: "glm-5.3",
        },
      ],
    });
    expect(next.slots[0].routeId).toBe("claude-fable-1");
  });

  it("lets a default target that references a slot id follow by position", () => {
    const next = assignSlotIds({
      defaultTarget: { kind: "slotId", value: "claude-fable-zhipu-glm" },
      slots: [
        {
          routeId: "claude-opus-deepseek",
          tier: "opus",
          providerId: "p-ds",
          upstreamModel: "a",
        },
        {
          routeId: "claude-fable-zhipu-glm",
          tier: "fable",
          providerId: "p-glm",
          upstreamModel: "b",
        },
      ],
    });
    expect(next.defaultTarget).toEqual({
      kind: "slotId",
      value: "claude-fable-1",
    });
  });

  it("clears the default target when the referenced slot was deleted, even if a sibling reclaims the id", () => {
    // A = sonnet-1 (deleted), B = sonnet-2 reclaims `claude-sonnet-1` after
    // re-numbering. Keeping the raw value would silently retarget the default
    // at B (a different vendor), so it must be cleared to force a reselection.
    const next = assignSlotIds({
      defaultTarget: { kind: "slotId", value: "claude-sonnet-1" },
      slots: [
        {
          routeId: "claude-sonnet-2",
          tier: "sonnet",
          providerId: "p-ds",
          upstreamModel: "a",
        },
      ],
    });
    expect(next.defaultTarget).toEqual({ kind: "slotId", value: "" });
  });

  it("clears the default target when the referenced slot was deleted and no sibling reclaims the id", () => {
    const next = assignSlotIds({
      defaultTarget: { kind: "slotId", value: "claude-fable-gone" },
      slots: [
        {
          routeId: "claude-sonnet-1",
          tier: "sonnet",
          providerId: "p-ds",
          upstreamModel: "a",
        },
      ],
    });
    expect(next.defaultTarget).toEqual({ kind: "slotId", value: "" });
  });
});

describe("groupSlotsByProvider / flattenProviderGroups", () => {
  const slot = (
    routeId: string,
    tier: "fable" | "opus" | "sonnet" | "haiku",
    providerId: string,
    upstreamModel: string,
  ) => ({ routeId, tier, providerId, upstreamModel });

  it("orders cards by each provider's first appearance (interleaved slots stay put)", () => {
    const cards = groupSlotsByProvider([
      slot("a", "fable", "p-glm", "glm-5.3"),
      slot("b", "opus", "p-ds", "deepseek-flash"),
      slot("c", "opus", "p-glm", "glm-5.3-flash"),
    ]);
    expect(cards.map((card) => card.providerId)).toEqual(["p-glm", "p-ds"]);
    expect(Object.keys(cards[0].rows)).toEqual(["fable", "opus"]);
    expect(cards[1].rows.opus?.upstreamModel).toBe("deepseek-flash");
  });

  it("keeps the first of a duplicate provider+tier pair (the fixed tier rows cannot create one)", () => {
    const cards = groupSlotsByProvider([
      slot("keep", "opus", "p-glm", "glm-5.3"),
      slot("drop", "opus", "p-glm", "glm-5.3-flash"),
    ]);
    expect(cards).toHaveLength(1);
    expect(cards[0].rows.opus?.routeId).toBe("keep");
  });

  it("returns no cards for an empty list", () => {
    expect(groupSlotsByProvider([])).toEqual([]);
  });

  it("flattens back in card order × fixed tier order (fable→opus→sonnet→haiku)", () => {
    const flat = flattenProviderGroups([
      { providerId: "p-ds", rows: { opus: slot("x", "opus", "p-ds", "d1") } },
      {
        providerId: "p-glm",
        rows: {
          haiku: slot("h", "haiku", "p-glm", "g3"),
          fable: slot("f", "fable", "p-glm", "g1"),
        },
      },
    ]);
    expect(flat.map((s) => [s.providerId, s.tier])).toEqual([
      ["p-ds", "opus"],
      ["p-glm", "fable"],
      ["p-glm", "haiku"],
    ]);
  });

  it("produces no slot for an unmapped tier (as many slots as tiers are mapped)", () => {
    const flat = flattenProviderGroups([
      {
        providerId: "p-glm",
        rows: { fable: slot("f", "fable", "p-glm", "g1") },
      },
    ]);
    expect(flat).toHaveLength(1);
  });

  it("exposes the same tier-row order the cards render", () => {
    expect(TIER_ROW_ORDER).toEqual(["fable", "opus", "sonnet", "haiku"]);
  });
});
