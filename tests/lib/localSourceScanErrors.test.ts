import { describe, expect, it } from "vitest";
import i18next from "i18next";
import { formatSkillError } from "@/lib/errors/skillErrorParser";
import en from "@/i18n/locales/en.json";
import zh from "@/i18n/locales/zh.json";
import zhTW from "@/i18n/locales/zh-TW.json";
import ja from "@/i18n/locales/ja.json";

describe.each(Object.entries({ en, zh, "zh-TW": zhTW, ja }))(
  "local source scan errors (%s)",
  (language, resource) => {
    it.each([
      "DIRECTORIES",
      "REPOSITORIES",
      "SKILLS",
      "ENTRIES",
      "BYTES",
      "ROOT",
      "FAILED",
    ])(
      "resolves %s with real locale resources and interpolated context",
      async (suffix) => {
        const instance = i18next.createInstance();
        await instance.init({
          lng: language,
          interpolation: { escapeValue: false },
          resources: { [language]: { translation: resource } },
        });
        const message = formatSkillError(
          JSON.stringify({
            code: `LOCAL_SOURCE_SCAN_${suffix}`,
            context: { limit: "100", path: "/tmp/projects" },
          }),
          instance.t.bind(instance),
          "common.error",
        );
        expect(message.description).not.toMatch(
          /skills\.|\{\{|LOCAL_SOURCE_SCAN/,
        );
        expect(message.description).not.toBe(
          resource.skills.error.unknownError,
        );
        if (suffix === "ROOT")
          expect(message.description).toContain("/tmp/projects");
        else if (suffix !== "FAILED")
          expect(message.description).toContain("100");
      },
    );
  },
);
