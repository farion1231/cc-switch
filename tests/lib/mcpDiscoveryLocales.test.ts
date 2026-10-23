import { createInstance } from "i18next";
import { describe, expect, it } from "vitest";
import en from "@/i18n/locales/en.json";
import zh from "@/i18n/locales/zh.json";
import zhTW from "@/i18n/locales/zh-TW.json";
import ja from "@/i18n/locales/ja.json";

describe("MCP discovery messages", () => {
  it.each(Object.entries({ en, zh, "zh-TW": zhTW, ja }))(
    "resolves the real %s resources and interpolates counts and app names",
    async (lng, translation) => {
      const i18n = createInstance();
      await i18n.init({ lng, resources: { [lng]: { translation } } });
      for (const key of [
        "available",
        "import",
        "readOnly",
        "partial",
        "failed",
        "retry",
      ]) {
        const fullKey = `mcpPage.discovery.${key}`;
        const message = i18n.t(fullKey, { count: 2, apps: "Codex" });
        expect(message).not.toBe(fullKey);
        expect(message).not.toContain("{{");
      }
      expect(i18n.t("mcpPage.discovery.available", { count: 2 })).toContain(
        "2",
      );
      expect(i18n.t("mcpPage.discovery.partial", { apps: "Codex" })).toContain(
        "Codex",
      );
    },
  );
});
