import { describe, expect, it } from "vitest";
import {
  mergeOmoConfigPreview,
  parseOmoOtherFieldsObject,
  type OmoGlobalConfig,
} from "@/types/omo";

const EMPTY_GLOBAL: OmoGlobalConfig = {
  id: "global",
  disabledAgents: [],
  disabledMcps: [],
  disabledHooks: [],
  disabledSkills: [],
  updatedAt: "2026-01-01T00:00:00.000Z",
};

describe("parseOmoOtherFieldsObject", () => {
  it("解析对象 JSON", () => {
    expect(parseOmoOtherFieldsObject('{ "foo": 1 }')).toEqual({ foo: 1 });
  });

  it("数组/字符串返回 undefined", () => {
    expect(parseOmoOtherFieldsObject('["a"]')).toBeUndefined();
    expect(parseOmoOtherFieldsObject('"hello"')).toBeUndefined();
  });

  it("非法 JSON 抛出异常", () => {
    expect(() => parseOmoOtherFieldsObject("{")).toThrow();
  });
});

describe("mergeOmoConfigPreview", () => {
  it("只合并 otherFields 的对象值，忽略数组", () => {
    const mergedFromArray = mergeOmoConfigPreview(
      EMPTY_GLOBAL,
      {},
      {},
      '["a", "b"]',
    );
    expect(mergedFromArray).toEqual({});

    const mergedFromObject = mergeOmoConfigPreview(
      EMPTY_GLOBAL,
      {},
      {},
      '{ "foo": "bar" }',
    );
    expect(mergedFromObject).toEqual({ foo: "bar" });
  });
});
