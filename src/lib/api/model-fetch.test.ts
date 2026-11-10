import { describe, it, expect, vi } from "vitest";
import { fetchModelsForConfig, modelFetchRequestHeaders } from "./model-fetch";

const invoke = vi.hoisted(() => vi.fn().mockResolvedValue([]));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

describe("Genesis model discovery", () => {
  it.each([
    "https://token.moi.matrixorigin.cn",
    "https://token.moi.matrixorigin.cn/v1",
  ])("fetches current chat models for %s", async (baseUrl) => {
    await fetchModelsForConfig(baseUrl, "test-key");
    expect(invoke).toHaveBeenLastCalledWith("fetch_models_for_config", {
      baseUrl,
      apiKey: "test-key",
      isFullUrl: undefined,
      modelsUrl: "https://token.moi.matrixorigin.cn/v1/models?type=chat",
      customUserAgent: undefined,
      apiFormat: undefined,
      requestHeaders: undefined,
    });
  });

  it("preserves an explicit catalog URL for other providers", async () => {
    await fetchModelsForConfig(
      "https://example.com/v1",
      "test-key",
      false,
      "https://example.com/custom-models",
    );
    expect(invoke).toHaveBeenLastCalledWith(
      "fetch_models_for_config",
      expect.objectContaining({
        modelsUrl: "https://example.com/custom-models",
      }),
    );
  });
});

describe("modelFetchRequestHeaders", () => {
  it("合法的 Header 覆盖 JSON 原样转为请求头（键名归一为小写）", () => {
    expect(
      modelFetchRequestHeaders('{\n  "cf-aig-authorization": "secret"\n}'),
    ).toEqual({ "cf-aig-authorization": "secret" });
    expect(modelFetchRequestHeaders('{"X-Custom":"a","x-lower":"b"}')).toEqual({
      "x-custom": "a",
      "x-lower": "b",
    });
  });

  it("空覆盖不携带请求头", () => {
    expect(modelFetchRequestHeaders("")).toBeUndefined();
    expect(modelFetchRequestHeaders("   \n  ")).toBeUndefined();
    expect(modelFetchRequestHeaders("{}")).toBeUndefined();
  });

  it("JSON 非法或结构不符合时静默回退（不阻断取模型）", () => {
    expect(modelFetchRequestHeaders("not json")).toBeUndefined();
    expect(modelFetchRequestHeaders('["array"]')).toBeUndefined();
    expect(modelFetchRequestHeaders('{"Name": 1}')).toBeUndefined();
    expect(modelFetchRequestHeaders('{"Bad Name!": "v"}')).toBeUndefined();
    expect(
      modelFetchRequestHeaders('{"dup": "a", "DUP": "b"}'),
    ).toBeUndefined();
  });

  it("本地代理托管头不允许借道模型列表请求", () => {
    expect(
      modelFetchRequestHeaders('{"authorization": "Bearer x"}'),
    ).toBeUndefined();
    expect(modelFetchRequestHeaders('{"x-api-key": "k"}')).toBeUndefined();
  });
});
