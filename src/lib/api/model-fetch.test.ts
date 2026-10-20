import { describe, it, expect } from "vitest";
import { modelFetchRequestHeaders } from "./model-fetch";

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
