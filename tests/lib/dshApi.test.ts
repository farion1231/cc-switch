import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { dshApi } from "@/lib/api/dsh";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

const invokeMock = vi.mocked(invoke);

describe("DSH API revisions", () => {
  beforeEach(() => {
    invokeMock.mockResolvedValue({ ok: true });
  });

  it("sends the settings revision with the default model selection", async () => {
    await dshApi.setDefaultModel(
      { provider: "deepseek", model: "deepseek-chat" },
      "settings-4",
    );

    expect(invokeMock).toHaveBeenCalledWith("dsh_set_default_model", {
      selection: { provider: "deepseek", model: "deepseek-chat" },
      expectedRevision: "settings-4",
    });
  });

  it("maps credential refs to the backend reference argument", async () => {
    await dshApi.setCredential({
      ref: "ACME_API_KEY",
      value: "secret",
      expectedRevision: "credentials-2",
    });
    await dshApi.unsetCredential("ACME_API_KEY", "credentials-3");

    expect(invokeMock).toHaveBeenNthCalledWith(1, "dsh_set_credential", {
      reference: "ACME_API_KEY",
      value: "secret",
      expectedRevision: "credentials-2",
    });
    expect(invokeMock).toHaveBeenNthCalledWith(2, "dsh_unset_credential", {
      reference: "ACME_API_KEY",
      expectedRevision: "credentials-3",
    });
  });

  it("uses Tauri lower-camel argument names for endpoint URLs", async () => {
    await dshApi.upsertNative({
      baseURL: "https://api.deepseek.com",
      models: [{ id: "deepseek-v4-flash" }],
    });
    await dshApi.createCustom({
      route: "gateway",
      api: "openai-completions",
      baseURL: "https://gateway.example/v1",
      models: [{ id: "model" }],
    });
    await dshApi.discoverModels({
      baseURL: "https://gateway.example/v1",
      api: "openai-completions",
    });

    expect(invokeMock).toHaveBeenNthCalledWith(1, "dsh_upsert_native", {
      baseUrl: "https://api.deepseek.com",
      models: [{ id: "deepseek-v4-flash" }],
      apiKeyEnv: undefined,
      expectedRevision: undefined,
    });
    expect(invokeMock).toHaveBeenNthCalledWith(2, "dsh_create_custom", {
      route: "gateway",
      displayName: undefined,
      api: "openai-completions",
      baseUrl: "https://gateway.example/v1",
      models: [{ id: "model" }],
      apiKeyEnv: undefined,
      expectedRevision: undefined,
    });
    expect(invokeMock).toHaveBeenNthCalledWith(3, "dsh_discover_models", {
      baseUrl: "https://gateway.example/v1",
      api: "openai-completions",
      apiKey: undefined,
    });
  });
});
