import type {
  AggregateRouteSlot,
  AggregateRoutes,
  AggregateTier,
  Provider,
} from "@/types";

/**
 * Slot id for the given ordinal within its tier: `claude-{tier}-{n}`.
 *
 * Deliberately free of any vendor information: Claude Desktop rejects model
 * ids that contain a vendor word (glm, deepseek, kimi, …) and drops the whole
 * list when it sees one, so a provider-derived slug would empty the picker.
 * The prefix keeps the id recognisable; the number keeps it unique.
 */
export function slotId(tier: AggregateTier, ordinal: number): string {
  return `claude-${tier}-${ordinal}`;
}

/** Whether the provider is an aggregate provider. */
export function isAggregateProvider(provider: Pick<Provider, "meta">): boolean {
  return Boolean(provider.meta?.aggregateRoutes);
}

/**
 * Derive a slot's label (the name shown in the picker): an explicit label
 * wins, otherwise fall back to the upstream model name.
 */
export function slotLabel(slot: {
  upstreamModel: string;
  label?: string;
}): string {
  const explicit = slot.label?.trim();
  return explicit && explicit.length > 0 ? explicit : slot.upstreamModel;
}

/** Whether the routing table can be saved (at least one slot, and a non-empty default target). */
export function canSaveAggregateRoutes(
  routes: AggregateRoutes | undefined | null,
): boolean {
  return Boolean(
    routes && routes.slots.length > 0 && routes.defaultTarget?.value.trim(),
  );
}

/**
 * Fixed order of the tier rows inside a provider card (strongest to weakest).
 * The persisted order of `slots[]` is the flatten of that order (see
 * `flattenProviderGroups`), which keeps id assignment deterministic.
 */
export const TIER_ROW_ORDER: readonly AggregateTier[] = [
  "fable",
  "opus",
  "sonnet",
  "haiku",
];

/** View shape of a provider card: one provider plus its mapped tiers (unmapped tiers have no key). */
export interface ProviderTierRows {
  providerId: string;
  rows: Partial<Record<AggregateTier, AggregateRouteSlot>>;
}

/**
 * Group the flat slot list by provider (used by the editor). Card order is the
 * order in which each provider first appears in the list. For duplicate
 * provider+tier pairs the first one wins — the fixed tier-row UI cannot produce
 * them; this only normalises legacy data, and it disappears on the next save.
 */
export function groupSlotsByProvider(
  slots: AggregateRouteSlot[],
): ProviderTierRows[] {
  const cards: ProviderTierRows[] = [];
  const byProvider = new Map<string, ProviderTierRows>();
  for (const slot of slots) {
    let card = byProvider.get(slot.providerId);
    if (!card) {
      card = { providerId: slot.providerId, rows: {} };
      byProvider.set(slot.providerId, card);
      cards.push(card);
    }
    if (!card.rows[slot.tier]) {
      card.rows[slot.tier] = slot;
    }
  }
  return cards;
}

/** Rebuild the flat slot array from card order × tier order (`TIER_ROW_ORDER`); unmapped tiers produce nothing. */
export function flattenProviderGroups(
  cards: ProviderTierRows[],
): AggregateRouteSlot[] {
  return cards.flatMap((card) => {
    const rows: AggregateRouteSlot[] = [];
    for (const tier of TIER_ROW_ORDER) {
      const slot = card.rows[tier];
      if (slot) rows.push(slot);
    }
    return rows;
  });
}

/**
 * Regenerate every slot's routeId (numbered per tier) and let a default target
 * that references a slot id follow by position.
 *
 * Call after slots are added, removed or re-tiered; this also serves to
 * **migrate legacy ids** (the earlier scheme embedded the provider name, which
 * Claude Desktop rejects as a group). If the default target references a slot
 * id, its new id is taken from its position **in this table**; when that slot
 * is gone (its old id is not in the table) the value is kept as-is and left
 * for save-time validation to reject, rather than silently retargeting it at
 * some other slot.
 */
export function assignSlotIds(routes: AggregateRoutes): AggregateRoutes {
  const ordinalByTier = new Map<AggregateTier, number>();
  const ids = routes.slots.map((slot) => {
    const ordinal = (ordinalByTier.get(slot.tier) ?? 0) + 1;
    ordinalByTier.set(slot.tier, ordinal);
    return slotId(slot.tier, ordinal);
  });

  const slots = routes.slots.map((slot, index) => ({
    ...slot,
    routeId: ids[index],
  }));

  let defaultTarget = routes.defaultTarget;
  if (defaultTarget.kind === "slotId") {
    const index = routes.slots.findIndex(
      (slot) => slot.routeId === defaultTarget.value,
    );
    if (index >= 0) {
      defaultTarget = { kind: "slotId", value: ids[index] };
    }
  }

  return { ...routes, slots, defaultTarget };
}
