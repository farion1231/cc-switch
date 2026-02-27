import { describe, expect, it } from "vitest";
import {
  buildOmoProfilePreview,
  parseOmoOtherFieldsObject,
} from "@/types/omo";

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

describe("buildOmoProfilePreview", () => {
  it("只合并 otherFields 的对象值，忽略数组", () => {
    const fromArray = buildOmoProfilePreview({}, {}, '["a", "b"]');
    expect(fromArray).toEqual({});

    const fromObject = buildOmoProfilePreview({}, {}, '{ "foo": "bar" }');
    expect(fromObject).toEqual({ foo: "bar" });
  });
});
