import { describe, expect, it } from "vitest";
import { codexBaseUrlEndpointHint } from "@/lib/api/model-fetch";

// 回归 #9432：模型发现与 Codex 实际请求路径不一致时给出可验证的提示。
// 仅在纯根地址（分歧可确证）时建议 {origin}/v1，绝不盲目补 /v1。
describe("codexBaseUrlEndpointHint", () => {
  it("suggests /v1 only for plain-root endpoints where divergence is provable", () => {
    expect(codexBaseUrlEndpointHint("https://api.example.com")).toBe(
      "https://api.example.com/v1",
    );
    expect(codexBaseUrlEndpointHint("https://api.example.com/")).toBe(
      "https://api.example.com/v1",
    );
    expect(codexBaseUrlEndpointHint("  https://api.example.com  ")).toBe(
      "https://api.example.com/v1",
    );
    expect(codexBaseUrlEndpointHint("http://localhost:8080")).toBe(
      "http://localhost:8080/v1",
    );
  });

  it("never touches custom paths, versioned endpoints, or overrides", () => {
    // 智谱 Coding Plan /v4：版本已在路径里，补 /v1 会打成 404
    expect(
      codexBaseUrlEndpointHint("https://open.bigmodel.cn/api/coding/paas/v4"),
    ).toBeNull();
    expect(codexBaseUrlEndpointHint("https://api.example.com/v1")).toBeNull();
    expect(codexBaseUrlEndpointHint("https://api.example.com/v1beta")).toBeNull();
    // Anthropic 兼容子路径
    expect(codexBaseUrlEndpointHint("https://api.deepseek.com/anthropic")).toBeNull();
    // 其它自定义路径
    expect(codexBaseUrlEndpointHint("https://api.example.com/api")).toBeNull();
    expect(
      codexBaseUrlEndpointHint("https://api.example.com/compatible-mode/v1"),
    ).toBeNull();
    // 查询串 / hash：无法确证，不动
    expect(codexBaseUrlEndpointHint("https://api.example.com/?foo=1")).toBeNull();
    expect(codexBaseUrlEndpointHint("https://api.example.com/#frag")).toBeNull();
  });

  it("returns null for invalid or empty input", () => {
    expect(codexBaseUrlEndpointHint("")).toBeNull();
    expect(codexBaseUrlEndpointHint("   ")).toBeNull();
    expect(codexBaseUrlEndpointHint("not a url")).toBeNull();
    expect(codexBaseUrlEndpointHint("ftp://api.example.com")).toBeNull();
  });
});
