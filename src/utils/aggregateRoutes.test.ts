import { describe, expect, it } from "vitest";
import type { AggregateRoutes, Provider } from "@/types";
import {
  configuredModelsOf,
  getAggregateRouteTargets,
  normalizeAggregateRoutes,
  validateAggregateRoutes,
} from "./aggregateRoutes";

function provider(
  id: string,
  name: string,
  env: Record<string, string> = {},
  routes?: AggregateRoutes,
): Provider {
  return {
    id,
    name,
    settingsConfig: { env },
    ...(routes ? { meta: { aggregateRoutes: routes } } : {}),
  };
}

describe("aggregate route helpers", () => {
  it("normalizes complete routes and rejects partial rows", () => {
    expect(
      normalizeAggregateRoutes({
        haiku: { providerId: " deepseek ", model: " v3 " },
        opus: { providerId: "", model: "" },
      }),
    ).toEqual({
      haiku: { providerId: "deepseek", model: "v3" },
    });

    expect(
      validateAggregateRoutes({
        sonnet: { providerId: "kimi", model: "" },
      }),
    ).toEqual({ ok: false, reason: "incomplete", tier: "sonnet" });
    expect(validateAggregateRoutes({})).toEqual({
      ok: false,
      reason: "empty",
    });
  });

  it("excludes self, official, and nested aggregate route targets", () => {
    const plain = provider("kimi", "Kimi");
    const official: Provider = {
      ...provider("official", "Anthropic"),
      category: "official",
    };
    const aggregate = provider(
      "aggregate",
      "Aggregate",
      {},
      {
        fable: { providerId: "kimi", model: "k3" },
      },
    );

    expect(
      getAggregateRouteTargets([plain, official, aggregate], "aggregate"),
    ).toEqual([plain]);
  });

  it("collects configured model names without duplicates", () => {
    expect(
      configuredModelsOf(
        provider("kimi", "Kimi", {
          ANTHROPIC_MODEL: "k3",
          ANTHROPIC_DEFAULT_FABLE_MODEL: "k3",
          ANTHROPIC_DEFAULT_HAIKU_MODEL: "k2.5",
        }),
      ),
    ).toEqual(["k3", "k2.5"]);
  });
});
