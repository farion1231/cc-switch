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

  it("filters models by an explicitly selected Copilot protocol", () => {
    const responsesOnly = model(["/v1/responses"]);
    const chatOnly = model(["/chat/completions"]);
    const both = model(["/responses", "/v1/chat/completions"]);
    expect(isCopilotModelSupportedByCodex(responsesOnly, "openai_chat")).toBe(
      false,
    );
    expect(isCopilotModelSupportedByCodex(chatOnly, "openai_responses")).toBe(
      false,
    );
    expect(
      isCopilotModelSupportedByCodex(responsesOnly, "openai_responses"),
    ).toBe(true);
    expect(isCopilotModelSupportedByCodex(chatOnly, "openai_chat")).toBe(true);
    expect(isCopilotModelSupportedByCodex(both, "openai_chat")).toBe(true);
    expect(isCopilotModelSupportedByCodex(both, "openai_responses")).toBe(true);
    expect(
      isCopilotModelSupportedByCodex(model(["/V1/RESPONSES/"]), "auto"),
    ).toBe(true);
  });
});
