import { describe, expect, it } from "vitest";
import { claudeDesktopProviderPresets } from "@/config/claudeDesktopProviderPresets";

describe("Claude Desktop OpenCode Go 预设", () => {
  const preset = claudeDesktopProviderPresets.find(
    (item) => item.name === "OpenCode Go",
  );

  it("默认走官方文档列出的 /inference/go/anthropic", () => {
    expect(preset?.baseUrl).toBe("https://opencode.ai/inference/go/anthropic");
  });
});

describe("Claude Desktop Kimi 预设的 1M 窗口声明", () => {
  for (const name of ["Kimi", "Kimi Global"]) {
    it(`${name} 的路由都声明 supports1m`, () => {
      const preset = claudeDesktopProviderPresets.find(
        (item) => item.name === name,
      );
      expect(preset).toBeDefined();
      // brandedRoutes 会把指向同一上游模型的三档去重成一条路由
      const routes = preset?.modelRoutes ?? [];
      expect(routes.length).toBeGreaterThan(0);
      expect(routes.every((route) => route.supports1m)).toBe(true);
    });
  }
});

describe("Claude Desktop 1M 模型预设的窗口声明", () => {
  for (const name of [
    "Volcengine Doubao",
    "Gemini Native",
    "StepFun",
    "Novita AI",
  ]) {
    it(`${name} 的路由都声明 supports1m`, () => {
      const preset = claudeDesktopProviderPresets.find(
        (item) => item.name === name,
      );
      const routes = preset?.modelRoutes ?? [];
      expect(routes.length).toBeGreaterThan(0);
      expect(routes.every((route) => route.supports1m)).toBe(true);
    });
  }
});
