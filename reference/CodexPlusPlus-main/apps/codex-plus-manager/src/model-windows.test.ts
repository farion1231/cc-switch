import assert from "node:assert";
import { describe, it } from "node:test";
import type { RelayProfile } from "./App.tsx";
import {
  buildModelWindows,
  modelWindowRowsFromProfile,
  modelWindowsMapToText,
  modelWindowsTextToMap,
  serializeModelWindowRows,
  mergeModelWindowRows,
} from "./model-windows.ts";

// 类型检查：确保 RelayProfile 包含 modelWindows 字段
const _profileTypeCheck: RelayProfile = {
  id: "test",
  name: "",
  model: "",
  baseUrl: "",
  upstreamBaseUrl: "",
  apiKey: "",
  protocol: "responses",
  relayMode: "official",
  officialMixApiKey: false,
  testModel: "",
  configContents: "",
  authContents: "",
  useCommonConfig: true,
  contextSelection: { mcpServers: [], skills: [], plugins: [] },
  contextSelectionInitialized: true,
  contextWindow: "",
  autoCompactLimit: "",
  modelList: "",
  modelWindows: "",
  userAgent: "",
};

void _profileTypeCheck;

describe("model-windows helpers", () => {
  it("modelWindowsMapToText 按 modelList 行顺序输出窗口文本", () => {
    assert.strictEqual(
      modelWindowsMapToText("a\nb\nc", '{"a":"1M","c":"200K"}'),
      "1M\n\n200K",
    );
  });

  it("modelWindowsMapToText 对非法 JSON 返回空字符串", () => {
    assert.strictEqual(modelWindowsMapToText("a\nb", "not-json"), "");
  });

  it("modelWindowsTextToMap 按行组装 model_windows map", () => {
    assert.strictEqual(
      modelWindowsTextToMap("a\nb\nc", "1M\n\n200K"),
      '{"a":"1M","c":"200K"}',
    );
  });

  it("modelWindowsTextToMap 对没有对应窗口的模型不写入 map", () => {
    assert.strictEqual(
      modelWindowsTextToMap("a\nb", "1M"),
      '{"a":"1M"}',
    );
  });

  it("buildModelWindows 行数一致时返回 modelWindows JSON", () => {
    const result = buildModelWindows("deepseek-v4-flash\ndeepseek-v4-pro", "1M\n");
    assert.strictEqual(result.ok, true);
    if (result.ok) {
      assert.strictEqual(result.modelWindows, '{"deepseek-v4-flash":"1M"}');
    }
  });

  it("buildModelWindows 行数不一致时返回错误", () => {
    const result = buildModelWindows("a\nb", "1M");
    assert.strictEqual(result.ok, false);
    if (!result.ok) {
      assert.ok(result.error.includes("2"));
      assert.ok(result.error.includes("1"));
    }
  });

  it("modelWindowRowsFromProfile 把模型和窗口合成同一组行", () => {
    assert.deepStrictEqual(
      modelWindowRowsFromProfile("a\nb\nc", '{"a":"1M","c":"200K"}'),
      [
        { model: "a", window: "1M" },
        { model: "b", window: "" },
        { model: "c", window: "200K" },
      ],
    );
  });

  it("serializeModelWindowRows 从行控件生成 modelList 和 modelWindows", () => {
    assert.deepStrictEqual(
      serializeModelWindowRows([
        { model: "a", window: "1M" },
        { model: "", window: "400K" },
        { model: "b", window: "" },
      ]),
      {
        modelList: "a\nb",
        modelWindows: '{"a":"1M"}',
      },
    );
  });

  it("mergeModelWindowRows 追加上游模型时跳过已有模型并保留窗口", () => {
    assert.deepStrictEqual(
      mergeModelWindowRows(
        [
          { model: "deepseek-v4-flash", window: "1M" },
          { model: "  ", window: "" },
        ],
        [
          { model: "deepseek-v4-flash", window: "" },
          { model: "deepseek-v4-pro", window: "" },
          { model: " deepseek-v4-pro ", window: "200K" },
        ],
      ),
      [
        { model: "deepseek-v4-flash", window: "1M" },
        { model: "deepseek-v4-pro", window: "" },
      ],
    );
  });
});
