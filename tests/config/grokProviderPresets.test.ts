import { describe, expect, it } from "vitest";
import { parse as parseToml } from "smol-toml";
import {
  generateGrokProfileConfig,
  grokProviderPresets,
} from "@/config/grokProviderPresets";
import {
  extractGrokApiBackend,
  extractGrokBaseUrl,
  setGrokApiBackend,
  setGrokBaseUrl,
} from "@/utils/grokConfigUtils";

describe("Grok Build provider presets", () => {
  it("provides official OAuth, Responses and Chat profile templates", () => {
    expect(
      grokProviderPresets.some(
        (preset) => preset.category === "official" && preset.config === "",
      ),
    ).toBe(true);
    const responses = grokProviderPresets.find(
      (preset) => preset.name === "xAI API (Responses)",
    );
    expect(extractGrokBaseUrl(responses?.config ?? "")).toBe(
      "https://api.x.ai/v1",
    );
    expect(extractGrokApiBackend(responses?.config ?? "")).toBe("responses");
    const parsed = parseToml(responses?.config ?? "") as any;
    expect(parsed.models.default).toBe("grok-4.5");
    expect(parsed.models.web_search).toBe("grok-4.5");
    expect(parsed.subagents.default_model).toBe("grok-4.5");
    expect(parsed.model["grok-4.5"].supports_backend_search).toBe(true);
  });

  it("updates Grok endpoint and every model backend without Codex wire_api", () => {
    const profile = `${generateGrokProfileConfig(
      "https://old.example/v1",
      "first.model",
    )}\n[model.second]\nmodel = "second"\napi_backend = "responses"\n`;
    const updated = setGrokApiBackend(
      setGrokBaseUrl(profile, "https://new.example/v1"),
      "chat_completions",
    );
    const parsed = parseToml(updated) as any;
    expect(parsed.endpoints.models_base_url).toBe("https://new.example/v1");
    expect(parsed.model["first.model"].api_backend).toBe("chat_completions");
    expect(parsed.model.second.api_backend).toBe("chat_completions");
    expect(updated).not.toContain("wire_api");
  });
});
