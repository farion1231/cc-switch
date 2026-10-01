import { describe, expect, it } from "vitest";
import { parse as parseToml } from "smol-toml";
import { getCodexCustomTemplate } from "@/config/codexTemplates";

describe("Codex custom templates", () => {
  it("starts keyless without official-login fallback or forced Goal mode", () => {
    const template = getCodexCustomTemplate();
    const parsed = parseToml(template.config) as {
      features?: { goals?: boolean };
      model_providers?: Record<string, { requires_openai_auth?: boolean }>;
    };

    expect(template.auth).toEqual({ OPENAI_API_KEY: "" });
    expect(parsed.features?.goals).toBeUndefined();
    expect(parsed.model_providers?.custom).toBeDefined();
    expect(parsed.model_providers?.custom.requires_openai_auth).toBe(false);
  });
});
