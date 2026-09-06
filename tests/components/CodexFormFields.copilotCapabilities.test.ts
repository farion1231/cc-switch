import { describe, expect, it } from "vitest";
import {
  isCopilotModelSupportedByCodex,
  resolveCopilotCatalogContextWindow,
} from "@/components/providers/forms/CodexFormFields";
import type { CopilotModel } from "@/lib/api/copilot";

function model(supportedEndpoints?: string[]): CopilotModel {
  return {
    id: "model",
    name: "Model",
    vendor: "vendor",
    model_picker_enabled: true,
    supported_endpoints: supportedEndpoints,
  };
}

describe("Codex Copilot capabilities", () => {
  it("accepts only advertised Responses or Chat endpoints", () => {
    expect(isCopilotModelSupportedByCodex(model(["/responses"]))).toBe(true);
    expect(
      isCopilotModelSupportedByCodex(model(["/v1/chat/completions?preview=1"])),
    ).toBe(true);
    expect(isCopilotModelSupportedByCodex(model(["/v1/messages"]))).toBe(false);
    expect(isCopilotModelSupportedByCodex(model())).toBe(false);
  });

  it("fills an empty catalog context window without overwriting an explicit value", () => {
    expect(resolveCopilotCatalogContextWindow("", 400_000)).toBe(400_000);
    expect(resolveCopilotCatalogContextWindow(undefined, 1_000_000)).toBe(
      1_000_000,
    );
    expect(resolveCopilotCatalogContextWindow(200_000, 400_000)).toBe(200_000);
  });
});
