import { describe, expect, it } from "vitest";
import {
  buildOmoProfilePreview,
  buildOmoSlimProfilePreview,
  OMO_SLIM_BUILTIN_AGENTS,
  OMO_SLIM_DISABLEABLE_AGENTS,
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

describe("buildOmoSlimProfilePreview", () => {
  it("保留 top-level council 配置，同时写入 council agent 模型", () => {
    const preview = buildOmoSlimProfilePreview(
      {
        council: { model: "openai/gpt-5.4-mini" },
      },
      '{ "council": { "default_preset": "default" }, "fallback": { "enabled": true } }',
    );

    expect(preview).toEqual({
      council: { default_preset: "default" },
      fallback: { enabled: true },
      agents: {
        council: { model: "openai/gpt-5.4-mini" },
      },
    });
  });
});

describe("OMO Slim metadata", () => {
  it("将 council 视为内置且可禁用的 agent", () => {
    expect(OMO_SLIM_BUILTIN_AGENTS).toContainEqual(
      expect.objectContaining({
        key: "council",
        display: "Council",
        group: "sub",
      }),
    );
    expect(OMO_SLIM_DISABLEABLE_AGENTS).toContainEqual({
      value: "council",
      label: "Council",
    });
  });
});
