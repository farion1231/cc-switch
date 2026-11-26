import type { ModelTrendStats } from "@/types/usage";

type ModelTrendMetric = keyof Omit<ModelTrendStats, "model">;

const MODEL_PALETTE = [
  "var(--heat-app-omp)",
  "var(--chart-2)",
  "var(--agent-gemini)",
  "var(--agent-pi)",
  "var(--agent-claude)",
  "var(--heat-app-grokbuild)",
  "var(--agent-opencode)",
  "var(--chart-usage)",
];

type ModelTrendPoint = {
  xKey: string;
  modelStats?: ModelTrendStats[];
};

type ModelTrendSeries = {
  // null denotes the aggregate series. An empty model ID is a real bucket.
  model: string | null;
  key: string;
  color: string;
  value: (point: ModelTrendPoint) => number;
};

function preferredColor(model: string): string {
  // FNV-1a gives each model a preferred color independent of rank and chart style.
  let hash = 2166136261;
  for (let i = 0; i < model.length; i++) {
    hash = Math.imul(hash ^ model.charCodeAt(i), 16777619);
  }
  return MODEL_PALETTE[(hash >>> 0) % MODEL_PALETTE.length];
}

export function buildModelTrendSeries(
  points: ModelTrendPoint[],
  metric: ModelTrendMetric,
): ModelTrendSeries[] {
  const totals = new Map<string, number>();
  for (const { modelStats } of points) {
    for (const stats of modelStats ?? []) {
      const { model } = stats;
      totals.set(model, (totals.get(model) ?? 0) + Math.abs(stats[metric]));
    }
  }
  const ranked = [...totals]
    .filter(([, value]) => value > 0)
    .sort(([a, aValue], [b, bValue]) => bValue - aValue || a.localeCompare(b));
  const top = ranked.slice(0, MODEL_PALETTE.length);
  const visibleModels = top.map(([model]) => model).sort();
  const availableColors = new Set(MODEL_PALETTE);
  const colors = new Map<string, string>();
  const collisions: string[] = [];
  // Reserve preferred colors first. Resolve collisions in model ID order so
  // a refresh that only changes the ranking does not recolor the chart.
  for (const model of visibleModels) {
    const color = preferredColor(model);
    if (availableColors.delete(color)) {
      colors.set(model, color);
    } else {
      collisions.push(model);
    }
  }
  for (const [index, color] of [...availableColors].entries()) {
    if (index === collisions.length) break;
    colors.set(collisions[index], color);
  }
  const series: ModelTrendSeries[] = top.map(([model]) => ({
    model,
    key: `model:${model}`,
    color: colors.get(model) ?? preferredColor(model),
    value: ({ modelStats }) =>
      modelStats?.find((stats) => stats.model === model)?.[metric] ?? 0,
  }));

  if (ranked.length > top.length) {
    const topModels = new Set(top.map(([model]) => model));
    const otherValues = new Map<string, number>();
    for (const { xKey, modelStats } of points) {
      let total = 0;
      for (const stats of modelStats ?? []) {
        const { model } = stats;
        if (!topModels.has(model)) total += stats[metric];
      }
      otherValues.set(xKey, total);
    }
    series.push({
      model: null,
      key: "other",
      color: "var(--text-3)",
      value: ({ xKey }) => otherValues.get(xKey) ?? 0,
    });
  }
  return series;
}
