import { describe, expect, it } from "vitest";
import ja from "@/i18n/locales/ja.json";

describe("Codex aggregation locale", () => {
  it("uses Japanese terminology for aggregation mode", () => {
    expect(ja.codexConfig.aggregationNoMappingError).toContain("集約モード");
    expect(ja.codexConfig.aggregationNoMappingError).not.toContain(
      "聚合モード",
    );
  });
});
