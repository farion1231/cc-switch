import type { ModelStats } from "@/types/usage";

export interface ModelDistributionRow extends ModelStats {
  tokenValue: number;
  cost: number;
  tokenShare: number | null;
  costShare: number | null;
  color: string;
}

export interface ModelDistribution {
  rows: ModelDistributionRow[];
  totalTokens: number;
  totalCost: number;
  invalidTokens: boolean;
  invalidCost: boolean;
}

function modelColor(model: string): string {
  // The hue depends only on the exact model name, never on row order or filters.
  let hash = 2166136261;
  for (let i = 0; i < model.length; i += 1) {
    hash = Math.imul(hash ^ model.charCodeAt(i), 16777619);
  }
  return `hsl(${(hash >>> 0) % 360}, 65%, 47%)`;
}

function validCost(value: string): number | null {
  if (!value.trim()) return null;
  const cost = Number(value);
  return Number.isFinite(cost) && cost >= 0 ? cost : null;
}

export function buildModelDistribution(stats: ModelStats[]): ModelDistribution {
  let totalTokens = 0;
  let totalCost = 0;
  let invalidTokens = false;
  let invalidCost = false;

  const rows = stats.map((stat): ModelDistributionRow => {
    const tokenValue =
      Number.isFinite(stat.totalTokens) && stat.totalTokens >= 0
        ? stat.totalTokens
        : 0;
    const parsedCost = validCost(stat.totalCost);
    if (tokenValue !== stat.totalTokens) invalidTokens = true;
    if (parsedCost === null) invalidCost = true;

    totalTokens += tokenValue;
    totalCost += parsedCost ?? 0;

    return {
      ...stat,
      tokenValue,
      cost: parsedCost ?? 0,
      tokenShare: null,
      costShare: null,
      color: modelColor(stat.model),
    };
  });

  if (!Number.isFinite(totalTokens)) invalidTokens = true;
  if (!Number.isFinite(totalCost)) invalidCost = true;

  return {
    rows: rows.map((row) => ({
      ...row,
      tokenShare:
        !invalidTokens &&
        totalTokens > 0 &&
        Number.isFinite(row.totalTokens) &&
        row.totalTokens >= 0
          ? row.tokenValue / totalTokens
          : null,
      costShare:
        !invalidCost && totalCost > 0 && validCost(row.totalCost) !== null
          ? row.cost / totalCost
          : null,
    })),
    totalTokens,
    totalCost,
    invalidTokens,
    invalidCost,
  };
}

export function formatModelShare(share: number | null): string {
  if (share === null) return "—";
  if (share > 0 && share < 0.001) return "<0.1%";
  return `${(share * 100).toFixed(1)}%`;
}
