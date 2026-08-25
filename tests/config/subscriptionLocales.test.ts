import { describe, expect, it } from "vitest";
import en from "@/i18n/locales/en.json";
import ja from "@/i18n/locales/ja.json";
import zh from "@/i18n/locales/zh.json";
import zhTW from "@/i18n/locales/zh-TW.json";

describe("Codex subscription detail locales", () => {
  it.each([
    ["zh", zh],
    ["zh-TW", zhTW],
    ["en", en],
    ["ja", ja],
  ])("keeps every reset-card key in %s", (_locale, messages) => {
    const subscription = messages.subscription;
    expect(subscription.plan).toBeTruthy();
    expect(subscription.membershipExpiryUnavailable).toBeTruthy();
    expect(subscription.membershipActiveUntil).toBeTruthy();
    expect(subscription.membershipRenewsAt).toBeTruthy();
    expect(subscription.membershipExpiresAt).toBeTruthy();
    expect(subscription.membershipExpired).toBeTruthy();
    expect(subscription.membershipValid).toBeTruthy();
    expect(subscription.membershipRenews).toBeTruthy();
    expect(subscription.membershipExpires).toBeTruthy();
    expect(subscription.membershipExpiredCompact).toBeTruthy();
    expect(subscription.membershipCountdown).toContain("{{time}}");
    expect(subscription.membershipSourceHint).toBeTruthy();
    expect(subscription.resetCards).toBeTruthy();
    expect(subscription.resetCard).toBeTruthy();
    expect(subscription.resetCardsAvailable).toContain("{{count}}");
    expect(subscription.resetCardDetailsUnavailable).toBeTruthy();
    expect(subscription.expiresAtWithCountdown).toContain("{{date}}");
    expect(subscription.expiresAtWithCountdown).toContain("{{time}}");
    expect(subscription.expiresAt).toContain("{{date}}");
    expect(subscription.noExpiry).toBeTruthy();
    expect(subscription.expiryUnknown).toBeTruthy();
  });
});
