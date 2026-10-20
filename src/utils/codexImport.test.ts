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
  it("keeps malformed declarations editable without assuming any reasoning support", () => {
    const malformed = {
      ...request,
      modelCatalog: { models: [{ model: "model-a", reasoningLevels: "high" }] },
    } as unknown as DeepLinkImportRequest;
    const result = applyCodexImportTemplate(malformed);
    expect(result.modelCatalog?.models[0].reasoningLevels).toEqual([]);
    expect(codexImportError(result)).toContain("思考等级");
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
      modelCatalog: undefined,
    });
    expect(result.modelCatalog?.models).toEqual([]);
    expect(codexImportError(result)).toBe("请添加模型列表");
    expect(
      codexImportError({
        ...request,
        modelCatalog: { models: [{ model: "model-a" }] },
      }),
    ).toBe("请逐个确认模型支持的思考等级");
  });
  it("requires a default model and complete connection values", () => {
    expect(codexImportError({ ...request, model: undefined })).toContain(
      "默认模型",
    );
    expect(codexImportError({ ...request, apiKey: "" })).toContain("API Key");
    expect(codexImportError({ ...request, endpoint: "" })).toContain(
      "API 地址",
    );
  });
});
