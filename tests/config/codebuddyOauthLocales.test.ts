import { describe, expect, it } from "vitest";
import en from "@/i18n/locales/en.json";
import ja from "@/i18n/locales/ja.json";
import tr from "@/i18n/locales/tr.json";
import zhTW from "@/i18n/locales/zh-TW.json";
import zh from "@/i18n/locales/zh.json";

const requiredKeys = [
  "codebuddyOauth.authStatus",
  "codebuddyOauth.accountCount",
  "codebuddyOauth.reauthRequired",
  "codebuddyOauth.notAuthenticated",
  "codebuddyOauth.selectAccount",
  "codebuddyOauth.selectAccountPlaceholder",
  "codebuddyOauth.useDefaultAccount",
  "codebuddyOauth.accounts",
  "codebuddyOauth.defaultAccount",
  "codebuddyOauth.expired",
  "codebuddyOauth.setAsDefault",
  "codebuddyOauth.removeAccount",
  "codebuddyOauth.addOrReauth",
  "codebuddyOauth.login",
  "codebuddyOauth.waitingForAuth",
  "codebuddyOauth.retry",
  "codebuddyOauth.logoutAll",
  "codebuddyOauth.loginRequired",
  "providerPreset.loginWith.codebuddy",
  "authCenter.group.codebuddyHelp",
  "authCenter.codebuddyReauthNote",
  "authCenter.codebuddyStatusLoadFailed",
  "settings.authCenter.description",
  "settings.authCenter.codebuddyOauthDescription",
  "subscription.codebuddyCredits",
  "subscription.codebuddyCreditsText",
] as const;

type TranslationTree = Record<string, unknown>;

function readTranslation(tree: TranslationTree, path: string): unknown {
  return path.split(".").reduce<unknown>((value, segment) => {
    if (typeof value !== "object" || value === null) return undefined;
    return (value as TranslationTree)[segment];
  }, tree);
}

const locales = [
  ["zh", zh],
  ["zh-TW", zhTW],
  ["en", en],
  ["ja", ja],
  ["tr", tr],
] as const;

describe("CodeBuddy OAuth locale coverage", () => {
  it.each(locales)(
    "defines every required key in %s",
    (_locale, translations) => {
      const missing = requiredKeys.filter((key) => {
        const value = readTranslation(translations, key);
        return typeof value !== "string" || value.trim().length === 0;
      });

      expect(missing).toEqual([]);
    },
  );

  it.each(locales)(
    "keeps the codebuddyOauth namespace aligned with zh in %s",
    (_locale, translations) => {
      const reference = new Set(
        Object.keys((zh as TranslationTree).codebuddyOauth as TranslationTree),
      );
      const actual = Object.keys(
        (translations as TranslationTree).codebuddyOauth as TranslationTree,
      );
      expect(actual.sort()).toEqual([...reference].sort());
    },
  );
});
