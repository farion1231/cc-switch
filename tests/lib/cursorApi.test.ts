import { describe, expect, it } from "vitest";
import {
  createCursorModelConfig,
  createCursorProviderChanges,
  groupCursorProvidersByEndpoint,
  normalizeCursorProviders,
  type CursorEndpoint,
  type CursorProvider,
} from "@/lib/api/cursor";

const endpoint: CursorEndpoint = {
  id: "endpoint-1",
  name: "Endpoint",
  type: "openai",
  baseURL: "https://api.example.com",
  apiKey: "secret",
  createdAt: 1,
};

const provider = (id: string, modelID = id): CursorProvider => ({
  id,
  name: id,
  settingsConfig: createCursorModelConfig({
    providerGroup: "Endpoint",
    endpointId: "endpoint-1",
    baseURL: "https://api.example.com",
    apiKey: "secret",
    modelID,
  }),
});

describe("normalizeCursorProviders", () => {
  it("为旧 Cursor 配置补齐新增字段和默认值", () => {
    const providers = normalizeCursorProviders({
      legacy: {
        id: "legacy",
        name: "Legacy Model",
        settingsConfig: {
          enabled: true,
          type: "openai",
          baseURL: "https://api.example.com",
          apiKey: "secret",
          modelID: "legacy-model",
        },
      },
    });

    expect(providers.legacy.settingsConfig.providerGroup).toBe("");
    expect(providers.legacy.settingsConfig.contextWindowTokens).toBe(0);
    expect(providers.legacy.settingsConfig.openAIEndpoint).toBe(
      "/v1/responses",
    );
  });

  it("保留已有提供商分类", () => {
    const providers = normalizeCursorProviders({
      current: {
        id: "current",
        name: "Current Model",
        settingsConfig: {
          providerGroup: "OpenRouter",
          type: "openai",
        },
      },
    });

    expect(providers.current.settingsConfig.providerGroup).toBe("OpenRouter");
  });
});

describe("createCursorProviderChanges", () => {
  it("根据原始 Provider ID 快照生成删除集合", () => {
    const original = [provider("keep"), provider("remove")];
    const updated = [{ ...original[0], name: "Updated" }, provider("new")];

    expect(createCursorProviderChanges(endpoint, original, updated)).toEqual({
      endpoint,
      upserts: updated,
      deletedProviderIds: ["remove"],
    });
  });

  it("允许删除 Endpoint 的最后一个模型", () => {
    expect(
      createCursorProviderChanges(endpoint, [provider("last")], []),
    ).toEqual({
      endpoint,
      upserts: [],
      deletedProviderIds: ["last"],
    });
  });

  it("模型为空时仍保留 Endpoint 分组", () => {
    expect(groupCursorProvidersByEndpoint([endpoint], [])).toEqual([
      { endpoint, providers: [] },
    ]);
  });
});
