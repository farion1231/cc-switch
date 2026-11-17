import { describe, expect, it } from "vitest";

import en from "@/i18n/locales/en.json";
import ja from "@/i18n/locales/ja.json";
import tr from "@/i18n/locales/tr.json";
import zh from "@/i18n/locales/zh.json";
import zhTW from "@/i18n/locales/zh-TW.json";
import { APP_IDS } from "@/config/appConfig";
import { skillAppEnabled, type SkillApps } from "@/lib/api/skills";

/**
 * `apps.<id>` 是动态模板 key（`t(`apps.${app}`)`），静态 `t("字面量")` 扫描与
 * TypeScript 都查不到 JSON 缺键，这里显式断言：每个 APP_ID 在 en 及 5 个核心
 * 语言都存在 `apps.<id>`，且值不是 key 本身（防裸 key 泄漏，如历史上缺失的
 * `apps.atomcode`）。
 */
const CORE_LANGUAGES: Array<[string, Record<string, unknown>]> = [
  ["zh", zh],
  ["zh-TW", zhTW],
  ["en", en],
  ["ja", ja],
  ["tr", tr],
];

describe.each(CORE_LANGUAGES)("apps.<id> i18n coverage (%s)", (_, locale) => {
  const apps = (locale as { apps?: Record<string, string> }).apps ?? {};

  it.each(APP_IDS)("has apps.%s", (appId) => {
    expect(apps[appId], `apps.${appId} missing in locale`).toBeTruthy();
    expect(apps[appId]).not.toBe(`apps.${appId}`);
  });
});

describe("skillAppEnabled safe indexing", () => {
  it("returns false for missing keys", () => {
    expect(skillAppEnabled({} as SkillApps, "atomcode")).toBe(false);
  });

  it("returns the stored flag for known keys", () => {
    expect(skillAppEnabled({ claude: true } as SkillApps, "claude")).toBe(true);
    expect(skillAppEnabled({ claude: false } as SkillApps, "claude")).toBe(
      false,
    );
  });
});
