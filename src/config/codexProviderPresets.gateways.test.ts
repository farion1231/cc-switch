import { describe, expect, it } from "vitest";
import { codexProviderPresets } from "./codexProviderPresets";
import { hasIcon } from "../icons/extracted";

// 自建/私有 LLM 网关（llm-gateway-go）预设的回归护栏。
// 本文件**只做静态断言，不发网络请求**；下面的事实基线是 2026-09-28 的 curl 级实测，
// 不是「经 codex 全链路」的断言：
// - 两个网关的 POST /v1/responses 都返回 200（claude-opus-5 / claude-sonnet-5 /
//   glm-5.2 / kimi-k3 / minimax-m3）→ wire_api 必须是 responses，apiFormat 必须是
//   openai_responses（走直连，不让本地路由做 Responses→Chat 转换，多一跳多一个失败面）。
// - 经 codex exec 的端到端结果会随网关负载大幅波动（拥塞时 503 / 长尾 60s+），
//   所以不能把「某个模型此刻通」写成断言；这里只锁「目录必须覆盖它们」这一条。
const GATEWAYS = [
  {
    name: "开轩 LLM 网关",
    baseUrl: "https://llm.kxpms.cn/v1",
    icon: "kxpms_gateway",
    iconColor: "#0EA5E9",
  },
  {
    name: "本地 LLM 网关 (8782)",
    baseUrl: "http://localhost:8782/v1",
    icon: "local_gateway_8782",
    iconColor: "#6366F1",
  },
] as const;

// Codex 支持的思考档位全集；预设里出现表外值时后端生成的 catalog 会被 codex 拒绝。
const CODEX_REASONING_LEVELS = new Set([
  "none",
  "minimal",
  "low",
  "medium",
  "high",
  "xhigh",
  "max",
  "ultra",
]);

describe("codexProviderPresets 自建网关预设", () => {
  it("两个网关预设都在，且指向各自 base_url", () => {
    for (const gateway of GATEWAYS) {
      const preset = codexProviderPresets.find((p) => p.name === gateway.name);
      expect(preset, gateway.name).toBeDefined();
      expect(preset!.config, gateway.name).toContain(
        `base_url = "${gateway.baseUrl}"`,
      );
      expect(preset!.config, gateway.name).toContain('wire_api = "responses"');
      expect(preset!.apiFormat, gateway.name).toBe("openai_responses");
      expect(preset!.category, gateway.name).toBe("custom");
    }
  });

  it("都不带静态 key（key 由用户在表单里填）", () => {
    for (const gateway of GATEWAYS) {
      const preset = codexProviderPresets.find((p) => p.name === gateway.name)!;
      expect(preset.auth, gateway.name).toEqual({ OPENAI_API_KEY: "" });
    }
  });

  it("模型目录覆盖实测可用的非 OpenAI 主力模型", () => {
    for (const gateway of GATEWAYS) {
      const preset = codexProviderPresets.find((p) => p.name === gateway.name)!;
      const models = (preset.modelCatalog ?? []).map((m) => m.model);
      expect(models, gateway.name).toEqual(
        expect.arrayContaining([
          "claude-opus-5",
          "claude-sonnet-5",
          "glm-5.2",
          "kimi-k3",
          "minimax-m3",
        ]),
      );
      // 目录非空且条目唯一（后端按 slug 生成 catalog，重复会互相覆盖）
      expect(new Set(models).size, gateway.name).toBe(models.length);
    }
  });

  it("本地网关补录了 SSOT 漏掉的 glm-5.2 / claude-opus-4-8 / minimax-m2.7", () => {
    const preset = codexProviderPresets.find(
      (p) => p.name === "本地 LLM 网关 (8782)",
    )!;
    const models = (preset.modelCatalog ?? []).map((m) => m.model);
    for (const model of ["glm-5.2", "claude-opus-4-8", "minimax-m2.7"]) {
      expect(models, model).toContain(model);
    }
  });

  it("默认模型是目录里第一个（非 OpenAI 阵营，且与 catalog 首行一致）", () => {
    for (const gateway of GATEWAYS) {
      const preset = codexProviderPresets.find((p) => p.name === gateway.name)!;
      expect(preset.config, gateway.name).toContain(
        `model = "${preset.modelCatalog![0].model}"`,
      );
      expect(preset.modelCatalog![0].model, gateway.name).toBe("claude-opus-5");
    }
  });

  it("两个预设都配了 UI 可解析的 SVG 图标 + 品牌色（避免纯文本列表）", () => {
    for (const gateway of GATEWAYS) {
      const preset = codexProviderPresets.find((p) => p.name === gateway.name)!;
      expect(preset.icon, gateway.name).toBe(gateway.icon);
      expect(preset.iconColor, gateway.name).toBe(gateway.iconColor);
      // 图标必须真的存在于 icons/extracted，避免「preset 引了一个未注册 icon」的
      // 静默 fallback 到首字母（曾经压在这条规则上的盲区）。
      expect(hasIcon(gateway.icon), gateway.name).toBe(true);
    }
  });

  it("思考档位与上下文窗口都在合法区间内", () => {
    for (const gateway of GATEWAYS) {
      const preset = codexProviderPresets.find((p) => p.name === gateway.name)!;
      for (const entry of preset.modelCatalog ?? []) {
        const label = `${gateway.name}/${entry.model}`;
        for (const level of entry.reasoningLevels ?? []) {
          expect(CODEX_REASONING_LEVELS.has(level), label).toBe(true);
        }
        if (entry.reasoningLevels) {
          expect(entry.reasoningLevels, label).toContain(
            entry.defaultReasoningLevel,
          );
        }
        expect(Number(entry.contextWindow), label).toBeGreaterThan(0);
        for (const modality of entry.inputModalities ?? ["text"]) {
          expect(["text", "image"], label).toContain(modality);
        }
      }
    }
  });
});
