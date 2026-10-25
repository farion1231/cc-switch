import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { beforeAll, describe, expect, it, vi } from "vitest";
import { ProviderForm } from "@/components/providers/forms/ProviderForm";
import { registerFormExtension } from "@/config/formExtensions";
import { CODEAGENT_3_AUTH_HEADERS } from "@/huawei/huaweiCodeAgent3Presets";
import { huaweiFormBehavior } from "@/huawei/formBehavior";
import { createTestQueryClient } from "../utils/testQueryClient";

vi.mock("@/components/JsonEditor", () => ({
  default: ({ value }: { value: string }) => <textarea readOnly value={value} />,
}));
vi.mock("@/components/providers/forms/hooks", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/components/providers/forms/hooks")>()),
  useCopilotAuth: () => ({ isAuthenticated: false, accounts: [] }),
  useCodexOauth: () => ({ isAuthenticated: false, accounts: [] }),
  useXaiOauth: () => ({ isAuthenticated: false, accounts: [] }),
}));

beforeAll(() => {
  registerFormExtension({
    providerType: "huawei_codeagent_3",
    ...huaweiFormBehavior,
    skipApiKeyRequired: true,
    defaultMetaFields: {
      skipTlsVerify: true,
      stripPathPrefix: "/v1",
      headerMode: "whitelist",
      allowedHeaders: ["content-type", "accept-encoding", "host"],
      extraAuthHeaders: CODEAGENT_3_AUTH_HEADERS,
    },
  });
});

describe("Claude Code Huawei provider metadata", () => {
  it("restores missing authentication defaults on edit without replacing existing metadata", async () => {
    const onSubmit = vi.fn();
    render(
      <QueryClientProvider client={createTestQueryClient()}>
        <ProviderForm
          appId="claude"
          providerId="huawei-fixture"
          initialData={{
            name: "Huawei CodeAgent 3.0",
            category: "cloud_provider",
            settingsConfig: {
              env: {
                ANTHROPIC_BASE_URL: "https://example.test/api/v2",
                ANTHROPIC_API_KEY: "PROXY_MANAGED",
              },
            },
            meta: {
              providerType: "huawei_codeagent_3",
              headerMode: "custom-mode",
            },
          }}
          submitLabel="save-provider"
          onSubmit={onSubmit}
          onCancel={vi.fn()}
        />
      </QueryClientProvider>,
    );
    fireEvent.click(screen.getByRole("button", { name: "save-provider" }));
    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    const meta = onSubmit.mock.calls[0][0].meta;
    expect(meta.extraAuthHeaders).toEqual(CODEAGENT_3_AUTH_HEADERS);
    expect(meta.skipTlsVerify).toBe(true);
    expect(meta.stripPathPrefix).toBe("/v1");
    expect(meta.headerMode).toBe("custom-mode");
  });
});
