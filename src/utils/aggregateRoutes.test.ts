import { describe, expect, it } from "vitest";
import type { AggregateRoutes, Provider } from "@/types";
import {
  CODEX_OFFICIAL_MODEL_SUGGESTIONS,
  codexConfiguredModelsOf,
  configuredModelsOf,
  customRoutesToRows,
  getAggregateRouteTargetIds,
  getAggregateRouteTargets,
  getCodexAggregateRouteConnection,
  hasAggregateRoutes,
  normalizeAggregateRoutes,
  rowsToCustomRoutes,
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
  it("normalizes complete routes and rejects partial rows (claude)", () => {
    expect(
      normalizeAggregateRoutes(
        {
          haiku: { providerId: " deepseek ", model: " v3 " },
          opus: { providerId: "", model: "" },
        },
        "claude",
      ),
    ).toEqual({
      haiku: { providerId: "deepseek", model: "v3" },
    });

    expect(
      validateAggregateRoutes(
        {
          sonnet: { providerId: "kimi", model: "" },
        },
        "claude",
      ),
    ).toEqual({ ok: false, reason: "incomplete", tier: "sonnet" });
    expect(validateAggregateRoutes({}, "claude")).toEqual({
      ok: false,
      reason: "empty",
    });
  });

  it("claude normalization drops custom entries", () => {
    expect(
      normalizeAggregateRoutes(
        {
          haiku: { providerId: "deepseek", model: "v3" },
          custom: { "gpt-5.5": { providerId: "kimi", model: "k2" } },
        },
        "claude",
      ),
    ).toEqual({
      haiku: { providerId: "deepseek", model: "v3" },
    });
  });

  it("treats any custom input as having aggregate routes", () => {
    expect(hasAggregateRoutes({})).toBe(false);
    expect(hasAggregateRoutes({ custom: {} })).toBe(false);
    expect(
      hasAggregateRoutes({
        custom: { "gpt-5.5": { providerId: "", model: "" } },
      }),
    ).toBe(true);
    expect(
      hasAggregateRoutes({
        custom: { "": { providerId: "kimi", model: "" } },
      }),
    ).toBe(true);
    expect(
      hasAggregateRoutes({
        custom: { "": { providerId: "", model: "k2" } },
      }),
    ).toBe(true);
  });

  it("normalizes codex custom routes: trims keys and drops tiers/partial entries", () => {
    expect(
      normalizeAggregateRoutes(
        {
          haiku: { providerId: "deepseek", model: "v3" },
          custom: {
            " gpt-5.5 ": { providerId: " kimi ", model: " k2 " },
            partial: { providerId: "kimi", model: "" },
            "": { providerId: "kimi", model: "k2" },
          },
        },
        "codex",
      ),
    ).toEqual({
      custom: { "gpt-5.5": { providerId: "kimi", model: "k2" } },
    });

    expect(
      normalizeAggregateRoutes(
        { haiku: { providerId: "deepseek", model: "v3" } },
        "codex",
      ),
    ).toEqual({});
  });

  it("validates codex custom routes: empty / incomplete / duplicate", () => {
    expect(validateAggregateRoutes({}, "codex")).toEqual({
      ok: false,
      reason: "empty",
    });

    expect(
      validateAggregateRoutes(
        { custom: { "gpt-5.5": { providerId: "kimi", model: "" } } },
        "codex",
      ),
    ).toEqual({ ok: false, reason: "incomplete", tier: "gpt-5.5" });

    // 重复 key 依赖表单传入的有序行态（Record 无法表达重复）
    expect(
      validateAggregateRoutes(
        {
          custom: { "gpt-5.5": { providerId: "kimi", model: "k2" } },
        },
        "codex",
        [
          { key: "gpt-5.5", providerId: "kimi", model: "k2" },
          { key: " gpt-5.5 ", providerId: "ds", model: "v3" },
        ],
      ),
    ).toEqual({ ok: false, reason: "duplicate", key: "gpt-5.5" });

    expect(
      validateAggregateRoutes(
        { custom: { "gpt-5.5": { providerId: "kimi", model: "k2" } } },
        "codex",
      ),
    ).toEqual({
      ok: true,
      routes: { custom: { "gpt-5.5": { providerId: "kimi", model: "k2" } } },
    });
  });

  it("round-trips custom rows and collects target ids across tiers and custom", () => {
    const rows = [
      { key: "gpt-5.5", providerId: "kimi", model: "k2" },
      { key: "o4-mini", providerId: "ds", model: "v3" },
    ];
    expect(customRoutesToRows(rowsToCustomRoutes(rows))).toEqual(rows);

    expect(
      getAggregateRouteTargetIds({
        haiku: { providerId: "kimi", model: "k3" },
        custom: {
          "gpt-5.5": { providerId: "kimi", model: "k2" },
          "o4-mini": { providerId: "ds", model: "v3" },
        },
      }),
    ).toEqual(["kimi", "ds"]);
    expect(getAggregateRouteTargetIds(null)).toEqual([]);
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

  it("offers codex official model suggestions owned by OpenAI", () => {
    expect(CODEX_OFFICIAL_MODEL_SUGGESTIONS.length).toBeGreaterThan(0);
    expect(
      CODEX_OFFICIAL_MODEL_SUGGESTIONS.every(
        (model) => model.ownedBy === "OpenAI" && model.id.trim() !== "",
      ),
    ).toBe(true);
    expect(CODEX_OFFICIAL_MODEL_SUGGESTIONS.map((model) => model.id)).toContain(
      "gpt-5.5",
    );
  });

  it("builds codex route connection from TOML config and auth", () => {
    const target: Provider = {
      id: "kimi",
      name: "Kimi",
      settingsConfig: {
        auth: { OPENAI_API_KEY: "sk-test" },
        config: [
          'model_provider = "kimi"',
          "",
          "[model_providers.kimi]",
          'base_url = "https://api.kimi.example/v1"',
          "",
        ].join("\n"),
      },
      meta: { isFullUrl: true, customUserAgent: "cc-switch" },
    };

    expect(getCodexAggregateRouteConnection(target)).toEqual({
      baseUrl: "https://api.kimi.example/v1",
      apiKey: "sk-test",
      isFullUrl: true,
      modelsUrl: undefined,
      customUserAgent: "cc-switch",
    });
  });

  it("falls back to empty strings when codex config/auth is missing", () => {
    expect(getCodexAggregateRouteConnection(provider("kimi", "Kimi"))).toEqual({
      baseUrl: "",
      apiKey: "",
      isFullUrl: undefined,
      modelsUrl: undefined,
      customUserAgent: undefined,
    });
  });

  it("collects codex catalog model names without duplicates", () => {
    const target: Provider = {
      id: "kimi",
      name: "Kimi",
      settingsConfig: {
        modelCatalog: {
          models: [
            { model: "k2" },
            { model: " k2.5 " },
            { model: "k2" },
            { model: "" },
          ],
        },
      },
    };

    expect(codexConfiguredModelsOf(target)).toEqual(["k2", "k2.5"]);
    expect(codexConfiguredModelsOf(provider("ds", "DeepSeek"))).toEqual([]);
  });
});
