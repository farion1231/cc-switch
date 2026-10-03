import { createInstance } from "i18next";
import { describe, expect, it } from "vitest";
import { formatSkillError } from "@/lib/errors/skillErrorParser";
import en from "@/i18n/locales/en.json";
import zh from "@/i18n/locales/zh.json";
import zhTW from "@/i18n/locales/zh-TW.json";
import ja from "@/i18n/locales/ja.json";

describe("formatSkillError", () => {
  it.each([
    ["en", en],
    ["zh", zh],
    ["zh-TW", zhTW],
    ["ja", ja],
  ] as const)(
    "localizes ambiguous skill names in %s",
    async (language, locale) => {
      const i18n = createInstance();
      await i18n.init({
        lng: language,
        resources: { [language]: { translation: locale } },
        interpolation: { escapeValue: false },
      });
      const error = JSON.stringify({
        code: "AMBIGUOUS_SKILL_NAME",
        context: { name: "browser-skill" },
      });
      for (const titleKey of ["skills.installFailed", "skills.updateFailed"]) {
        const result = formatSkillError(error, i18n.t, titleKey);
        expect(result.title).toBe(i18n.t(titleKey));
        expect(result.description).toBe(
          locale.skills.error.ambiguousSkillName.replace(
            "{{name}}",
            "browser-skill",
          ),
        );
        expect(result.description).not.toContain("AMBIGUOUS_SKILL_NAME");
      }
      expect(formatSkillError("network failure", i18n.t).description).toBe(
        "network failure",
      );
    },
  );
});
