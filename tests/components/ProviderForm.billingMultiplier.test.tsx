import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import { ProviderForm } from "@/components/providers/forms/ProviderForm";
import { isValidBillingMultiplier } from "@/components/providers/forms/BillingMultiplierField";
import { createTestQueryClient } from "../utils/testQueryClient";

vi.mock("@/components/JsonEditor", () => ({
  default: ({ value }: { value: string }) => (
    <textarea readOnly value={value} />
  ),
}));

describe("provider billing multiplier", () => {
  it.each(["-1", "NaN", "Infinity", "1001", "1e2", "0.5oops"])(
    "rejects %s",
    (value) => expect(isValidBillingMultiplier(value)).toBe(false),
  );
  it.each(["", "0", "0.5", "1", "1.5", "1000"])("accepts %s", (value) =>
    expect(isValidBillingMultiplier(value)).toBe(true),
  );

  it.each(["0.25", ""])(
    "saves an edited multiplier (%s) without dropping unrelated metadata",
    async (value) => {
      const submit = vi.fn();
      render(
        <QueryClientProvider client={createTestQueryClient()}>
          <ProviderForm
            appId="claude"
            providerId="relay"
            initialData={{
              name: "Relay",
              settingsConfig: {
                env: {
                  ANTHROPIC_AUTH_TOKEN: "test",
                  ANTHROPIC_BASE_URL: "https://example.com",
                },
              },
              meta: { billingMultiplier: "0.5", isPartner: true },
            }}
            submitLabel="save-provider"
            onSubmit={submit}
            onCancel={vi.fn()}
          />
        </QueryClientProvider>,
      );
      const input = await screen.findByDisplayValue("0.5");
      fireEvent.change(input, { target: { value } });
      fireEvent.click(screen.getByRole("button", { name: "save-provider" }));
      await waitFor(() => expect(submit).toHaveBeenCalledOnce());
      expect(submit.mock.calls[0][0].meta.billingMultiplier).toBe(
        value || undefined,
      );
      expect(submit.mock.calls[0][0].meta.isPartner).toBe(true);
    },
  );
});
