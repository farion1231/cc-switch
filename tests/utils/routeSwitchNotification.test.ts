import { describe, expect, it } from "vitest";
import { createInstance } from "i18next";
import { routeSwitchNotification } from "@/utils/routeSwitchNotification";
import type { RouteSwitchResult } from "@/types/proxy";
import type { AppId } from "@/lib/api/types";
import en from "@/i18n/locales/en.json";
import zh from "@/i18n/locales/zh.json";
import tw from "@/i18n/locales/zh-TW.json";
import ja from "@/i18n/locales/ja.json";
import tr from "@/i18n/locales/tr.json";

async function translate(locale = "zh") {
  const i18n = createInstance();
  await i18n.init({
    lng: locale,
    resources: {
      en: { translation: en },
      zh: { translation: zh },
      "zh-TW": { translation: tw },
      ja: { translation: ja },
      tr: { translation: tr },
    },
    interpolation: { escapeValue: false },
  });
  return i18n.t;
}

describe("route switch notifications", () => {
  it.each([
    "codex",
    "gemini",
    "grokbuild",
    "claude",
    "claude-desktop",
  ] as const)(
    "does not require restarting %s when only the upstream changes",
    async (app) => {
      const t = await translate();
      expect(
        routeSwitchNotification(
          { kind: "route", clientConfigChanged: false },
          app,
          "B",
          t,
        ),
      ).toBe("路由已切换至 B，后续新请求将使用该供应商。");
    },
  );

  it.each(["codex", "gemini", "grokbuild", "claude-desktop"] as const)(
    "retains the %s reload hint for a changed route or aggregation contract",
    async (app) => {
      const t = await translate();
      for (const kind of ["route", "stack"] as const) {
        expect(
          routeSwitchNotification(
            { kind, clientConfigChanged: true },
            app,
            "B",
            t,
          ),
        ).toContain("客户端配置也已更新；重启");
      }
    },
  );

  it("does not promise immediate use of changed client-side credentials", async () => {
    const t = await translate();
    expect(
      routeSwitchNotification(
        { kind: "route", clientConfigChanged: true },
        "codex",
        "Official B",
        t,
      ),
    ).toBe(
      "路由目标已设为 Official B。客户端配置也已更新；重启 Codex 以加载更新后的配置。",
    );
  });

  it("does not request a restart for Claude Code's live config reload", async () => {
    const t = await translate();
    expect(
      routeSwitchNotification(
        { kind: "stack", clientConfigChanged: true },
        "claude",
        "B",
        t,
      ),
    ).toBe("聚合默认供应商已设为 B；已选聚合模型仍使用各自的供应商。");
  });

  it("does not claim that a remembered choice is already routing", async () => {
    const t = await translate();
    expect(
      routeSwitchNotification(
        { kind: "remembered", clientConfigChanged: false },
        "codex",
        "B",
        t,
      ),
    ).toBe("已记住供应商 B，客户端接入路由或聚合后使用。");
  });

  it.each(["en", "zh", "zh-TW", "ja", "tr"])(
    "has complete translations and interpolation for %s",
    async (locale) => {
      const t = await translate(locale);
      for (const kind of [
        "remembered",
        "route",
        "stack",
      ] as RouteSwitchResult["kind"][]) {
        const message = routeSwitchNotification(
          { kind, clientConfigChanged: kind !== "remembered" },
          "codex" as AppId,
          "Provider B",
          t,
        );
        expect(message).toContain("Provider B");
        expect(message).not.toMatch(/mode\.toast|\{\{|undefined/);
        if (kind !== "remembered") expect(message).toContain("Codex");
      }
    },
  );
});
