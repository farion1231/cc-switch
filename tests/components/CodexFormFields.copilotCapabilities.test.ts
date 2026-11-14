import { describe, expect, it } from "vitest";
import {
  isCopilotModelSupportedByCodex,
  resolveCopilotReportedPromptLimit,
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

  it("treats a reported prompt limit as authoritative and otherwise preserves the fallback", () => {
    expect(resolveCopilotReportedPromptLimit("", 400_000)).toBe(400_000);
    expect(resolveCopilotReportedPromptLimit(undefined, 1_000_000)).toBe(
      1_000_000,
    );
    expect(resolveCopilotReportedPromptLimit(200_000, 400_000)).toBe(400_000);
    expect(resolveCopilotReportedPromptLimit(200_000, undefined)).toBe(200_000);
  });

  it("rejects an absent capabilities field", () => {
    expect(isCopilotModelSupportedByCodex(model())).toBe(false);
  });
});
