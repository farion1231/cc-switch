import { describe, expect, it } from "vitest";
import type { Provider } from "@/types";
import { toProviderUpdateInput } from "./providers";

describe("toProviderUpdateInput", () => {
  it("removes hydrated endpoints and row-state fields from update payloads", () => {
    const hydrated: Provider = {
      id: "endpoint-provider",
      name: "Endpoint provider",
      settingsConfig: { env: { API_KEY: "secret" } },
      createdAt: 1_700_000_000,
      sortIndex: 7,
      inFailoverQueue: true,
      meta: {
        custom_endpoints: {
          "https://one.example": {
            url: "https://one.example",
            addedAt: null,
          },
        },
        usage_script: {
          enabled: true,
          language: "javascript",
          code: "{}",
        },
      },
    };

    const update = toProviderUpdateInput(hydrated);

    expect(update).not.toHaveProperty("createdAt");
    expect(update).not.toHaveProperty("sortIndex");
    expect(update).not.toHaveProperty("inFailoverQueue");
    expect(update.meta).not.toHaveProperty("custom_endpoints");
    expect(update.meta?.usage_script).toEqual(hydrated.meta?.usage_script);
    expect(
      hydrated.meta?.custom_endpoints?.["https://one.example"].addedAt,
    ).toBeNull();
  });
});
