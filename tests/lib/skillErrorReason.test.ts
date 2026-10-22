import { describe, expect, it } from "vitest";
import type { TFunction } from "i18next";
import { skillErrorReason } from "@/lib/errors/skillErrorParser";

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

  it("keeps the first line of an unstructured error", () => {
    expect(skillErrorReason("connection reset\nstack…", t)).toBe(
      "connection reset",
    );
    expect(skillErrorReason("", t)).toBe("common.error");
  });
});
