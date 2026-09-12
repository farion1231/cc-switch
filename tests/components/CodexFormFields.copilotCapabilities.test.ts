import { describe, expect, it } from "vitest";
import {
  isCopilotModelSupportedByCodex,
  resolveCopilotCatalogContextWindow,
} from "@/components/providers/forms/CodexFormFields";
import type { CopilotModel } from "@/lib/api/copilot";
import type { CodexCopilotApiFormat } from "@/types";
import endpointCases from "../fixtures/copilot-endpoint-cases.json";

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
  it.each(endpointCases)(
    "matches the backend endpoint contract: $name",
    ({ endpoints, formats }) => {
      const selections: CodexCopilotApiFormat[] = [
        "auto",
        "openai_responses",
        "openai_chat",
      ];
      for (const format of selections) {
        expect(isCopilotModelSupportedByCodex(model(endpoints), format)).toBe(
          formats.includes(format),
        );
      }
    },
  );

  it("fills an empty catalog context window without overwriting an explicit value", () => {
    expect(resolveCopilotCatalogContextWindow("", 400_000)).toBe(400_000);
    expect(resolveCopilotCatalogContextWindow(undefined, 1_000_000)).toBe(
      1_000_000,
    );
    expect(resolveCopilotCatalogContextWindow(200_000, 400_000)).toBe(200_000);
  });

  it("rejects an absent capabilities field", () => {
    expect(isCopilotModelSupportedByCodex(model())).toBe(false);
  });
});
