import { describe, expect, it } from "vitest";

import { getSkillRepoFailureReasonKey } from "@/lib/errors/skillErrorParser";
import en from "@/i18n/locales/en.json";
import ja from "@/i18n/locales/ja.json";
import zhTW from "@/i18n/locales/zh-TW.json";
import zh from "@/i18n/locales/zh.json";

describe("getSkillRepoFailureReasonKey", () => {
  it.each([
    ["download timed out", "timeout"],
    ["error sending request: dns error", "network"],
    [
      "error sending request for url (https://github.com/owner/repo/archive/refs/heads/main.zip)",
      "network",
    ],
    [
      JSON.stringify({ code: "DOWNLOAD_FAILED", context: { status: "404" } }),
      "notFound",
    ],
    ["HTTP 429 rate limit", "accessLimited"],
    ["NO_SKILLS_IN_ZIP", "noSkills"],
    ["SKILL_DIR_NOT_FOUND: installed-skill", "noSkills"],
  ])("classifies %s", (error, reason) => {
    expect(getSkillRepoFailureReasonKey(error)).toBe(
      `skills.repo.failureReason.${reason}`,
    );
  });

  it("keeps failure reason translations under the existing skills.repo object", () => {
    for (const locale of [en, ja, zhTW, zh]) {
      expect(locale.skills.repo.failureReason.timeout).toBeTruthy();
      expect(locale.skills.repo.failureReason.network).toBeTruthy();
    }
  });
});
