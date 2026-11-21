import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";
import type { TFunction } from "i18next";
import {
  getUsageProviderLabel,
  usageProviderTitle,
} from "@/components/usage/providerLabel";

const t = ((key: string, options?: Record<string, unknown>) =>
  options?.app ? `${key}:${options.app}` : key) as unknown as TFunction;

describe("getUsageProviderLabel", () => {
  it("translates session-log placeholder names with the full app name", () => {
    expect(getUsageProviderLabel("Claude (Session)", t)).toEqual({
      label: "usage.sessionProvider.label:Claude Code",
      shortLabel: "usage.sessionProvider.short",
      hint: "usage.sessionProvider.hint",
    });
    expect(getUsageProviderLabel("Gemini (Session)", t).label).toBe(
      "usage.sessionProvider.label:Gemini CLI",
    );
    expect(getUsageProviderLabel("Oh My Pi (Session)", t).label).toBe(
      "usage.sessionProvider.label:Oh My Pi",
    );
  });

  it("translates every placeholder name the backend can return", () => {
    // 后端 provider_name_coalesce 里的占位名，新增应用时漏了前端对照表就会原样显示英文全名
    const rust = readFileSync(
      resolve(__dirname, "../../src-tauri/src/services/usage_stats.rs"),
      "utf8",
    );
    const names = [...rust.matchAll(/THEN '([^']+ \(Session\))'/g)].map(
      (m) => m[1],
    );
    expect(names).toContain("Antigravity (Session)");
    expect(names).toContain("DeepSeek Harness (Session)");
    for (const name of names) {
      expect(getUsageProviderLabel(name, t).shortLabel).toBe(
        "usage.sessionProvider.short",
      );
    }
    // 认得出走的是官方账号登录（OAuth）的占位名，单独一种说明
    expect(rust).toContain("THEN 'DeepSeek Harness (OAuth)'");
    expect(getUsageProviderLabel("DeepSeek Harness (OAuth)", t)).toEqual({
      label: "usage.sessionAccountProvider.label:DeepSeek Harness",
      shortLabel: "usage.sessionAccountProvider.short",
      hint: "usage.sessionAccountProvider.hint",
    });
  });

  it("keeps real provider names as-is", () => {
    expect(getUsageProviderLabel("DeepSeek", t)).toEqual({
      label: "DeepSeek",
      shortLabel: "DeepSeek",
    });
  });

  it("falls back to unknown provider for empty names", () => {
    expect(getUsageProviderLabel("", t).label).toBe("usage.unknownProvider");
    expect(getUsageProviderLabel(undefined, t).label).toBe(
      "usage.unknownProvider",
    );
  });

  it("appends the hint to the hover title only for placeholders", () => {
    expect(usageProviderTitle({ label: "A", hint: "B" })).toBe("A\nB");
    expect(usageProviderTitle({ label: "A" })).toBe("A");
  });
});
