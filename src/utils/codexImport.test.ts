import { describe, expect, it } from "vitest";
import { applyCodexImportTemplate, codexImportError } from "./codexImport";
import type { DeepLinkImportRequest } from "@/lib/api/deeplink";

const request: DeepLinkImportRequest = {
  version: "v1",
  resource: "provider",
  app: "codex",
  enabled: true,
  endpoint: "https://relay.example/v1",
  apiKey: "test-key",
  model: "model-a",
  modelCatalog: {
    models: [{ model: "model-a", reasoningLevels: ["low", "max", "ultra"] }],
  },
};
describe("fixed Codex import policy", () => {
  it("leaves a legacy Codex link unchanged when it has no model catalog", () => {
    const legacy = { ...request, modelCatalog: undefined };
    expect(applyCodexImportTemplate(legacy)).toBe(legacy);
    expect(legacy.enabled).toBe(true);
    expect(legacy.usageScript).toBeUndefined();
  });
  it("keeps malformed declarations editable without assuming any reasoning support", () => {
    const malformed = {
      ...request,
      modelCatalog: { models: [{ model: "model-a", reasoningLevels: "high" }] },
    } as unknown as DeepLinkImportRequest;
    const result = applyCodexImportTemplate(malformed);
    expect(result.modelCatalog?.models[0].reasoningLevels).toEqual([]);
    expect(codexImportError(result)).toBe("codexCustomization.error.levels");
  });
  it("preserves explicitly imported declarations and script while disabling activation", () => {
    const incoming = { ...request, usageScript: btoa("({ imported: true })") };
    const result = applyCodexImportTemplate(incoming);
    expect(result.usageScript).toBe(incoming.usageScript);
    expect(result.modelCatalog).toEqual(incoming.modelCatalog);
    expect(result.enabled).toBe(false);
    expect(incoming.enabled).toBe(true);
    expect(codexImportError(result)).toBeUndefined();
  });
  it("uses the user's script without carrying credentials from another provider", () => {
    const result = applyCodexImportTemplate(request);
    expect(atob(result.usageScript!)).toContain('"Bearer {{apiKey}}"');
    expect(atob(result.usageScript!)).toContain("response?.quota?.remaining");
    expect(result.apiKey).toBe("test-key");
  });
  it("does not assume missing models or levels", () => {
    const result = applyCodexImportTemplate({
      ...request,
      model: undefined,
      modelCatalog: { models: [] },
    });
    expect(result.modelCatalog?.models).toEqual([]);
    expect(codexImportError(result)).toBe("codexCustomization.error.models");
    expect(
      codexImportError({
        ...request,
        modelCatalog: { models: [{ model: "model-a" }] },
      }),
    ).toBe("codexCustomization.error.levels");
  });
  it("requires a default model and complete connection values", () => {
    expect(codexImportError({ ...request, model: undefined })).toBe(
      "codexCustomization.error.defaultModel",
    );
    expect(codexImportError({ ...request, apiKey: "" })).toBe(
      "codexCustomization.error.apiKey",
    );
    expect(codexImportError({ ...request, endpoint: "" })).toBe(
      "codexCustomization.error.endpoint",
    );
  });
});
