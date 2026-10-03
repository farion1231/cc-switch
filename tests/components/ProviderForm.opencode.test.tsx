import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import {
  ProviderForm,
  type ProviderFormValues,
} from "@/components/providers/forms/ProviderForm";
import { opencodeProviderPresets } from "@/config/opencodeProviderPresets";
import { createTestQueryClient } from "../utils/testQueryClient";

describe("ProviderForm OpenCode presets", () => {
  it("persists the selected preset usage metadata", async () => {
    const onSubmit = vi.fn();
    const queryClient = createTestQueryClient();

    render(
      <QueryClientProvider client={queryClient}>
        <ProviderForm
          appId="opencode"
          submitLabel="save-provider"
          onSubmit={onSubmit}
          onCancel={vi.fn()}
        />
      </QueryClientProvider>,
    );

    fireEvent.click(screen.getByRole("button", { name: /OpenCode Go/ }));
    fireEvent.change(screen.getByLabelText("API Key"), {
      target: { value: "test-api-key" },
    });
    fireEvent.change(screen.getByLabelText(/providerKey/i), {
      target: { value: "opencode-go" },
    });
    fireEvent.click(screen.getByRole("button", { name: "save-provider" }));

    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    const submitted = onSubmit.mock.calls[0][0] as ProviderFormValues;
    const preset = opencodeProviderPresets.find(
      (item) => item.name === "OpenCode Go",
    );

    expect(submitted.presetId).toBe(
      `opencode-${opencodeProviderPresets.indexOf(preset!)}`,
    );
    expect(submitted.meta?.usage_script).toEqual(preset?.meta?.usage_script);

    const script = JSON.parse(submitted.meta!.usage_script!.code);
    expect(script.request.url).toBe("{{baseUrl}}/usage");
  });
});
