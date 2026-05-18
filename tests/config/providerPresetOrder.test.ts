import { describe, expect, it } from "vitest";
import { providerPresets } from "@/config/claudeProviderPresets";
import { claudeDesktopProviderPresets } from "@/config/claudeDesktopProviderPresets";
import { codexProviderPresets } from "@/config/codexProviderPresets";
import { opencodeProviderPresets } from "@/config/opencodeProviderPresets";
import { openclawProviderPresets } from "@/config/openclawProviderPresets";
import { hermesProviderPresets } from "@/config/hermesProviderPresets";

const namesOf = (presets: Array<{ name: string }>) =>
  presets.map((preset) => preset.name);

const expectInOrder = (names: string[], expected: string[]) => {
  const indexes = expected.map((name) => names.indexOf(name));

  expect(indexes).not.toContain(-1);
  expect(indexes).toEqual(expected.map((_, index) => indexes[0] + index));
};

describe("provider preset order", () => {
  it("Claude 预设按合作伙伴优先顺序排列", () => {
    expectInOrder(namesOf(providerPresets), [
      "Shengsuanyun",
      "PatewayAI",
      "火山Agentplan",
      "BytePlus",
      "DouBaoSeed",
    ]);
  });

  it("Claude Desktop 预设按合作伙伴优先顺序排列", () => {
    expectInOrder(namesOf(claudeDesktopProviderPresets), [
      "Shengsuanyun",
      "PatewayAI",
      "火山Agentplan",
      "BytePlus",
      "DouBaoSeed",
    ]);
  });

  it("Claude Desktop 预设包含官方登录入口", () => {
    expect(claudeDesktopProviderPresets[0]).toMatchObject({
      name: "Claude Desktop Official",
      category: "official",
      baseUrl: "",
      mode: "direct",
    });
  });

  it("Codex 预设把 PatewayAI 放在胜算云后面", () => {
    expectInOrder(namesOf(codexProviderPresets), ["Shengsuanyun", "PatewayAI"]);
  });

  it("OpenCode 预设把火山、BytePlus、DouBaoSeed 放在胜算云后面", () => {
    expectInOrder(namesOf(opencodeProviderPresets), [
      "Shengsuanyun",
      "火山Agentplan",
      "BytePlus",
      "DouBaoSeed",
    ]);
  });

  it("OpenClaw 预设把火山、BytePlus、DouBaoSeed 放在胜算云后面", () => {
    expectInOrder(namesOf(openclawProviderPresets), [
      "Shengsuanyun",
      "火山Agentplan",
      "BytePlus",
      "DouBaoSeed",
    ]);
  });

  it("Hermes 预设把火山、BytePlus、DouBaoSeed 放在胜算云后面", () => {
    expectInOrder(namesOf(hermesProviderPresets), [
      "Shengsuanyun",
      "火山Agentplan",
      "BytePlus",
      "DouBaoSeed",
    ]);
  });
});
