import { describe, expect, it } from "vitest";
import en from "@/i18n/locales/en.json";
import ja from "@/i18n/locales/ja.json";
import zhTW from "@/i18n/locales/zh-TW.json";
import zh from "@/i18n/locales/zh.json";

const requiredKeys = [
  "xaiOauth.authStatus",
  "xaiOauth.accountCount",
  "xaiOauth.reauthRequired",
  "xaiOauth.notAuthenticated",
  "xaiOauth.selectAccount",
  "xaiOauth.selectAccountPlaceholder",
  "xaiOauth.useDefaultAccount",
  "xaiOauth.accounts",
  "xaiOauth.defaultAccount",
  "xaiOauth.expired",
  "xaiOauth.setAsDefault",
  "xaiOauth.removeAccount",
  "xaiOauth.addOrReauth",
  "xaiOauth.login",
  "xaiOauth.waitingForAuth",
  "xaiOauth.enterCode",
  "xaiOauth.retry",
  "xaiOauth.logoutAll",
  "xaiOauth.loginRequired",
  "managedAuth.selectedAccountNeedsReauth",
  "managedAuth.selectedAccountUnavailable",
  "providerForm.providerKeyStatusLoading",
  "settings.authCenter.xaiOauthDescription",
] as const;

type TranslationTree = Record<string, unknown>;

function readTranslation(tree: TranslationTree, path: string): unknown {
  return path.split(".").reduce<unknown>((value, segment) => {
    if (typeof value !== "object" || value === null) return undefined;
    return (value as TranslationTree)[segment];
  }, tree);
}

describe("xAI OAuth locale coverage", () => {
  it.each([
    ["zh", zh],
    ["zh-TW", zhTW],
    ["en", en],
    ["ja", ja],
  ])("defines every required key in %s", (_locale, translations) => {
    const missing = requiredKeys.filter((key) => {
      const value = readTranslation(translations, key);
      return typeof value !== "string" || value.trim().length === 0;
    });

    expect(missing).toEqual([]);
  });
});
