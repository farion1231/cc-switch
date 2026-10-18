import { describe, expect, it } from "vitest";
import { createInstance, type TFunction } from "i18next";
import { skillErrorReason } from "@/lib/errors/skillErrorParser";
import en from "@/i18n/locales/en.json";
import zh from "@/i18n/locales/zh.json";
import zhTW from "@/i18n/locales/zh-TW.json";
import ja from "@/i18n/locales/ja.json";

const t = ((key: string, options?: Record<string, unknown>) =>
  options && Object.keys(options).length
    ? `${key}:${JSON.stringify(options)}`
    : key) as unknown as TFunction;

describe("skillErrorReason", () => {
  it("uses the HTTP explanation for rate-limited downloads", () => {
    const error = JSON.stringify({
      code: "DOWNLOAD_FAILED",
      context: { status: "403" },
      suggestion: "http403",
    });
    expect(skillErrorReason(error, t)).toBe("skills.error.http403");
  });

  it("uses the error code text with its context otherwise", () => {
    const error = JSON.stringify({
      code: "DOWNLOAD_TIMEOUT",
      context: { owner: "a", name: "b", timeout: "60" },
      suggestion: "checkNetwork",
    });
    expect(skillErrorReason(error, t)).toBe(
      'skills.error.downloadTimeout:{"owner":"a","name":"b","timeout":"60"}',
    );
  });

  it("tells an oversized download apart from the extraction limit", () => {
    // #8421：下载体超限曾复用 ARCHIVE_TOO_LARGE，被说成「解压后超过上限」
    const error = JSON.stringify({
      code: "ARCHIVE_DOWNLOAD_TOO_LARGE",
      context: { limit_mb: "512" },
    });
    expect(skillErrorReason(error, t)).toBe(
      'skills.error.archiveDownloadTooLarge:{"limit_mb":"512"}',
    );
  });

  it.each([
    ["DISCOVERY_TREE_TRUNCATED", "skills.error.discoveryTreeTruncated"],
    ["DISCOVERY_METADATA_INVALID", "skills.error.discoveryMetadataInvalid"],
    ["DISCOVERY_LIMIT_EXCEEDED", "skills.error.discoveryLimitExceeded"],
  ])("explains %s without falling back to an unknown error", (code, key) => {
    expect(skillErrorReason(JSON.stringify({ code, context: {} }), t)).toBe(
      key,
    );
  });

  it.each([
    ["en", en],
    ["zh", zh],
    ["zh-TW", zhTW],
    ["ja", ja],
  ] as const)(
    "translates discovery failures in %s",
    async (locale, resource) => {
      const localized = createInstance();
      await localized.init({
        lng: locale,
        fallbackLng: false,
        resources: { [locale]: { translation: resource } },
        interpolation: { escapeValue: false },
      });
      const reasons = [
        "DISCOVERY_TREE_TRUNCATED",
        "DISCOVERY_METADATA_INVALID",
        "DISCOVERY_LIMIT_EXCEEDED",
      ].map((code) =>
        skillErrorReason(
          JSON.stringify({ code, context: { path: "untrusted-path" } }),
          localized.t,
        ),
      );
      for (const reason of reasons) {
        expect(reason.trim()).not.toBe("");
        expect(reason).not.toBe(localized.t("skills.error.unknownError"));
        expect(reason).not.toMatch(/skills\.error\.|\{\{|untrusted-path/);
      }
      expect(new Set(reasons).size).toBe(3);
    },
  );

  it("keeps the first line of an unstructured error", () => {
    expect(skillErrorReason("connection reset\nstack…", t)).toBe(
      "connection reset",
    );
    expect(skillErrorReason("", t)).toBe("common.error");
  });
});
