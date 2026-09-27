import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { UniversalProviderFormModal } from "@/components/universal/UniversalProviderFormModal";
import { findPresetByType } from "@/config/universalProviderPresets";

describe("UniversalProviderFormModal CodeBuddy preset", () => {
  it("prefills the full Chat Completions endpoint without appending /v1", () => {
    render(
      <UniversalProviderFormModal
        isOpen
        initialPreset={findPresetByType("codebuddy")}
        onClose={() => {}}
        onSave={() => {}}
      />,
    );

    expect(document.querySelector("#baseUrl")).toHaveValue(
      "https://copilot.tencent.com/v2/chat/completions",
    );
    expect(document.body.textContent).toContain("Hermes");
  });
});
