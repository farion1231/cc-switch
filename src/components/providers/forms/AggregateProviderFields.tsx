import { useMemo, useState } from "react";
import { Download, Loader2, Plus, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { ImeSafeInput } from "@/components/ui/ime-safe-input";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { ModelDropdown } from "./shared/ModelDropdown";
import type { FetchedModel } from "@/lib/api/model-fetch";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useTranslation } from "react-i18next";
import type {
  AggregateRouteSlot,
  AggregateRoutes,
  AggregateTier,
  DefaultTarget,
  Provider,
} from "@/types";
import {
  assignSlotIds,
  canSaveAggregateRoutes,
  flattenProviderGroups,
  groupSlotsByProvider,
  slotLabel,
  TIER_ROW_ORDER,
  type ProviderTierRows,
} from "@/utils/aggregateRoutes";

interface Props {
  value: AggregateRoutes;
  onChange: (next: AggregateRoutes) => void;
  /** Ordinary providers that can be targeted (excludes the aggregate provider itself and official providers, to prevent nesting / credential-less targets) */
  candidates: Provider[];
  /** Models already fetched for a target provider; cached by provider id and shared across that card's tiers */
  modelsForProvider: (providerId: string) => FetchedModel[];
  /** Provider id currently being fetched; while non-null every fetch button shows as loading */
  fetchingProviderId: string | null;
  onFetchModels: (provider: Provider) => void;
}

/** Deep-copy the card array (row objects are only read and replaced wholesale, so copying two levels is enough). */
const cloneCards = (cards: ProviderTierRows[]): ProviderTierRows[] =>
  cards.map((card) => ({
    providerId: card.providerId,
    rows: { ...card.rows },
  }));

export function AggregateProviderFields({
  value,
  onChange,
  candidates,
  modelsForProvider,
  fetchingProviderId,
  onFetchModels,
}: Props) {
  const { t } = useTranslation();

  const providerNameById = new Map(candidates.map((p) => [p.id, p.name]));
  const labelOf = (slot: AggregateRouteSlot) => slotLabel(slot);
  const nameOf = (id: string) => providerNameById.get(id) ?? id;

  // The editor renders "provider cards" while storage stays a flat slots[] (the
  // grouping is purely a view concern, so the backend and persistence are
  // unchanged). A provider that has been picked but has no tier mapped yet
  // leaves no trace in slots, so pendingProviders remembers that empty card —
  // otherwise it would vanish on the next render.
  const [pendingProviders, setPendingProviders] = useState<string[]>([]);

  const grouped = useMemo(
    () => groupSlotsByProvider(value.slots),
    [value.slots],
  );
  const cards = useMemo(() => {
    const groupedIds = new Set(grouped.map((card) => card.providerId));
    const extras = pendingProviders
      .filter((id) => !groupedIds.has(id))
      .map((id): ProviderTierRows => ({ providerId: id, rows: {} }));
    return [...grouped, ...extras];
  }, [grouped, pendingProviders]);

  // Recompute every slot id on each change so the preview always equals the
  // value that gets submitted. assignSlotIds renumbers per tier from scratch and
  // lets a default target referencing a slot id follow by position, so a
  // reference never dangles after a slot is added or removed (if the target
  // slot is gone the value is kept and save-time validation rejects it).
  const commit = (next: AggregateRoutes) => onChange(assignSlotIds(next));

  /** Commit the edited card list: cards that have rows are flattened back into
   *  slots, and empty cards (a provider picked but no tier mapped yet) are kept
   *  in pendingProviders. */
  const commitCards = (next: ProviderTierRows[]) => {
    const hasRows = (card: ProviderTierRows) =>
      TIER_ROW_ORDER.some((tier) => card.rows[tier]);
    setPendingProviders(
      next.filter((card) => !hasRows(card)).map((c) => c.providerId),
    );
    commit({
      ...value,
      slots: flattenProviderGroups(next.filter(hasRows)),
    });
  };

  const setCardProvider = (cardIndex: number, providerId: string) => {
    const next = cloneCards(cards);
    const card = next[cardIndex];
    card.providerId = providerId;
    // Each row's slot carries its own providerId (redundant in flat storage), so
    // changing the card header has to rewrite them too.
    for (const tier of TIER_ROW_ORDER) {
      const slot = card.rows[tier];
      if (slot) card.rows[tier] = { ...slot, providerId };
    }
    commitCards(next);
  };

  /** The upstream-model input decides whether the tier's slot exists: a
   *  non-empty value maps the tier, clearing it removes the slot ("as many
   *  slots as tiers are mapped"). New slots leave routeId empty for the
   *  assignSlotIds call inside commit to number. */
  const setUpstream = (cardIndex: number, tier: AggregateTier, raw: string) => {
    const next = cloneCards(cards);
    const card = next[cardIndex];
    const existing = card.rows[tier];
    if (raw.trim() === "") {
      delete card.rows[tier];
    } else if (existing) {
      card.rows[tier] = { ...existing, upstreamModel: raw };
    } else {
      card.rows[tier] = {
        routeId: "",
        tier,
        providerId: card.providerId,
        upstreamModel: raw,
        supports1m: false,
      };
    }
    commitCards(next);
  };

  const patchRow = (
    cardIndex: number,
    tier: AggregateTier,
    patch: Partial<AggregateRouteSlot>,
  ) => {
    const next = cloneCards(cards);
    const slot = next[cardIndex].rows[tier];
    if (!slot) return;
    next[cardIndex].rows[tier] = { ...slot, ...patch };
    commitCards(next);
  };

  const removeCard = (cardIndex: number) =>
    commitCards(cards.filter((_, i) => i !== cardIndex));

  const addCard = () => {
    const used = new Set(cards.map((card) => card.providerId));
    const free = candidates.find((p) => !used.has(p.id));
    if (!free) return;
    commitCards([...cards, { providerId: free.id, rows: {} }]);
  };

  return (
    <section className="space-y-4">
      <header className="space-y-1">
        <h3 className="text-sm font-medium">{t("aggregate.title")}</h3>
        <p className="text-xs text-muted-foreground">{t("aggregate.hint")}</p>
      </header>

      {!canSaveAggregateRoutes(value) && (
        <p className="text-xs text-destructive">{t("aggregate.notSaveable")}</p>
      )}

      {/* The default target is required, so it sits *above* the provider cards:
          further down it gets pushed out of view as the list grows, and it
          rendered as a blank dropdown with no placeholder, which left users
          stuck. Its options come from the providers and the slots, so picking a
          provider first is still valid while no slot is mapped. */}
      <div className="space-y-1">
        <Label className="text-xs">{t("aggregate.defaultTarget")}</Label>
        <Select
          value={
            value.defaultTarget.kind === "providerId"
              ? `provider:${value.defaultTarget.value}`
              : `slot:${value.defaultTarget.value}`
          }
          onValueChange={(v) => {
            const [kind, ...rest] = v.split(":");
            const target: DefaultTarget =
              kind === "provider"
                ? { kind: "providerId", value: rest.join(":") }
                : { kind: "slotId", value: rest.join(":") };
            onChange({ ...value, defaultTarget: target });
          }}
        >
          <SelectTrigger className="h-8">
            <SelectValue
              placeholder={t("aggregate.defaultTargetPlaceholder")}
            />
          </SelectTrigger>
          <SelectContent>
            {candidates.map((p) => (
              <SelectItem key={p.id} value={`provider:${p.id}`}>
                {p.name}
              </SelectItem>
            ))}
            {value.slots.map((s, index) => (
              <SelectItem
                key={s.routeId || `slot-${index}`}
                value={`slot:${s.routeId}`}
              >
                {labelOf(s)} ({s.routeId})
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <p className="text-xs text-muted-foreground">
          {t("aggregate.defaultTargetHint")}
        </p>
      </div>

      <div className="space-y-3">
        {cards.map((card, cardIndex) => {
          const usedElsewhere = new Set(
            cards.filter((_, i) => i !== cardIndex).map((c) => c.providerId),
          );
          const selectable = candidates.filter((p) => !usedElsewhere.has(p.id));
          const target = candidates.find((p) => p.id === card.providerId);
          const fetched = modelsForProvider(card.providerId);
          return (
            <div
              key={card.providerId || `card-${cardIndex}`}
              className="space-y-2 rounded-md border border-border-default p-3"
            >
              {/* Card header: one provider per card. The model list is fetched
                  once per provider and the cache is shared across the card. */}
              <div className="flex items-center gap-2">
                <Select
                  value={card.providerId}
                  onValueChange={(v) => setCardProvider(cardIndex, v)}
                >
                  <SelectTrigger className="h-8 flex-1">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {selectable.map((p) => (
                      <SelectItem key={p.id} value={p.id}>
                        {p.name}
                      </SelectItem>
                    ))}
                    {/* Legacy data may reference a provider that was deleted or
                        is already used by another card: fall back to showing it
                        so a blank picker does not read as "not configured". */}
                    {!selectable.some((p) => p.id === card.providerId) && (
                      <SelectItem value={card.providerId}>
                        {nameOf(card.providerId)}
                      </SelectItem>
                    )}
                  </SelectContent>
                </Select>
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  className="h-8 shrink-0 gap-1"
                  disabled={!target || fetchingProviderId !== null}
                  onClick={() => target && onFetchModels(target)}
                >
                  {fetchingProviderId === card.providerId ? (
                    <Loader2 className="h-3.5 w-3.5 animate-spin" />
                  ) : (
                    <Download className="h-3.5 w-3.5" />
                  )}
                  {t("providerForm.fetchModels")}
                </Button>
                <Button
                  type="button"
                  variant="ghost"
                  size="icon"
                  className="h-8 w-8 shrink-0"
                  onClick={() => removeCard(cardIndex)}
                >
                  <Trash2 className="h-3.5 w-3.5" />
                </Button>
              </div>

              {/* Four fixed tier rows (strongest to weakest). A non-empty
                  upstream model maps the tier, clearing it removes the slot, and
                  unmapped rows are disabled. The three columns line up: the
                  fetch button used to sit in the "upstream model" label row,
                  which left the two input columns misaligned. */}
              {TIER_ROW_ORDER.map((tier) => {
                const slot = card.rows[tier];
                const mapped = Boolean(slot?.upstreamModel.trim());
                const switchId = `agg-1m-${cardIndex}-${tier}`;
                return (
                  <div key={tier} className="flex items-center gap-2">
                    <span className="w-14 shrink-0 text-xs text-muted-foreground">
                      {tier}
                    </span>
                    <div className="flex min-w-0 flex-1 gap-1">
                      <Input
                        className="h-8 min-w-0 flex-1"
                        value={slot?.upstreamModel ?? ""}
                        placeholder={t("aggregate.upstreamModelPlaceholder")}
                        onChange={(e) =>
                          setUpstream(cardIndex, tier, e.target.value)
                        }
                      />
                      {fetched.length > 0 && (
                        <ModelDropdown
                          models={fetched}
                          onSelect={(id) => setUpstream(cardIndex, tier, id)}
                        />
                      )}
                    </div>
                    <ImeSafeInput
                      className="h-8 min-w-0 flex-1"
                      disabled={!mapped}
                      placeholder={
                        slot ? labelOf(slot) : t("aggregate.displayName")
                      }
                      value={slot?.label ?? ""}
                      // Left blank it is not persisted; the form fills in the
                      // default "provider · upstream model" on submit (see the
                      // submit path in ClaudeDesktopProviderForm).
                      onValueChange={(v) =>
                        patchRow(cardIndex, tier, {
                          label: v.trim() ? v : undefined,
                        })
                      }
                    />
                    <div className="flex shrink-0 items-center gap-1.5">
                      <Switch
                        id={switchId}
                        disabled={!mapped}
                        checked={Boolean(slot?.supports1m)}
                        onCheckedChange={(v) =>
                          patchRow(cardIndex, tier, { supports1m: v })
                        }
                      />
                      <Label htmlFor={switchId} className="text-xs">
                        {t("aggregate.supports1m")}
                      </Label>
                    </div>
                    {/* The slot id preview takes a fixed width; unmapped rows
                        stay blank so the columns keep lining up. */}
                    <code className="w-28 shrink-0 truncate text-right text-[10px] text-muted-foreground">
                      {slot?.routeId}
                    </code>
                  </div>
                );
              })}
            </div>
          );
        })}
      </div>

      <Button
        type="button"
        variant="outline"
        size="sm"
        className="h-8 gap-1.5"
        disabled={
          candidates.length > 0 &&
          new Set(cards.map((card) => card.providerId)).size >=
            candidates.length
        }
        onClick={addCard}
      >
        <Plus className="h-3.5 w-3.5" />
        {t("aggregate.addProvider")}
      </Button>
    </section>
  );
}
