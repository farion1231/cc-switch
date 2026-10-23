import { describe, expect, it } from "vitest";
import { HERMES_DEFAULT_CONFIG } from "@/components/providers/forms/hooks/useHermesFormState";

// Regression test for issue #4011: UI-created custom Hermes providers were
// saved without `api_mode` when the user never touched the API Mode dropdown,
// leaving Hermes to guess the protocol from the URL — which only works for a
// handful of official endpoints.
describe("HERMES_DEFAULT_CONFIG", () => {
  it("ships with a valid default api_mode", () => {
    const config = JSON.parse(HERMES_DEFAULT_CONFIG);

    expect(config.api_mode).toBe("chat_completions");
  });
});
