import { renderHook, waitFor } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { hermesProviderPresets } from "@/config/hermesProviderPresets";
import { openclawProviderPresets } from "@/config/openclawProviderPresets";
import { useProviderCategory } from "@/components/providers/forms/hooks/useProviderCategory";

describe("useProviderCategory", () => {
  it("detects Hermes preset categories", async () => {
    const index = hermesProviderPresets.findIndex(
      (preset) => preset.name === "OpenAI Official",
    );

    expect(index).toBeGreaterThanOrEqual(0);

    const { result } = renderHook(() =>
      useProviderCategory({
        appId: "hermes",
        selectedPresetId: `hermes-${index}`,
        isEditMode: false,
      }),
    );

    await waitFor(() => expect(result.current.category).toBe("official"));
  });

  it("detects OpenClaw preset categories", async () => {
    const index = openclawProviderPresets.findIndex(
      (preset) => preset.name === "Shengsuanyun",
    );

    expect(index).toBeGreaterThanOrEqual(0);

    const { result } = renderHook(() =>
      useProviderCategory({
        appId: "openclaw",
        selectedPresetId: `openclaw-${index}`,
        isEditMode: false,
      }),
    );

    await waitFor(() => expect(result.current.category).toBe("aggregator"));
  });
});
