import { describe, expect, it } from "vitest";

import { codexProviderPresets } from "./codexProviderPresets";

// DeepSeek 官方端点已把 `deepseek-v4-flash` 路由到识图的 V4.1 Flash，全局纯文本
// 名单因此不再收录该 id（#7283）。千帆 / 腾讯 Token Plan 托管的仍是纯文本的 V4
// 部署（千帆 Coding Plan 文档明写附图 400），必须靠预设行的显式声明兜住，
// 否则会 fail-open 成"可附图"。
const textOnlyDeepSeekHosts = [
  "Baidu Qianfan Token Plan",
  "Tencent Token Plan Enterprise Pro",
  "Tencent Token Plan Enterprise Pro (Intl)",
];

describe("third-party Token Plan DeepSeek V4 rows stay text-only", () => {
  it.each(textOnlyDeepSeekHosts)("%s declares text-only", (name) => {
    const preset = codexProviderPresets.find((p) => p.name === name);
    expect(preset, name).toBeDefined();
    const rows = (preset!.modelCatalog ?? []).filter((row) =>
      row.model.startsWith("deepseek-v4-"),
    );
    expect(rows.length, `${name} carries DeepSeek V4 rows`).toBeGreaterThan(0);
    for (const row of rows) {
      expect(row.inputModalities, `${name}/${row.model}`).toEqual(["text"]);
    }
  });

  it("the official DeepSeek preset keeps fail-open (no modality override)", () => {
    const preset = codexProviderPresets.find((p) => p.name === "DeepSeek");
    expect(preset).toBeDefined();
    for (const row of preset!.modelCatalog ?? []) {
      expect(row.inputModalities, row.model).toBeUndefined();
    }
  });
});
