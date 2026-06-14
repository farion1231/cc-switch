import { describe, expect, it } from "vitest";

import { getSkillRepoFailureReasonKey } from "@/lib/errors/skillErrorParser";

describe("getSkillRepoFailureReasonKey", () => {
  it("groups repository failures into simple user-facing reasons", () => {
    expect(
      getSkillRepoFailureReasonKey(
        '{"code":"DOWNLOAD_TIMEOUT","context":{"timeout":"60"}}',
      ),
    ).toBe("skills.repo.failureReason.timeout");
    expect(
      getSkillRepoFailureReasonKey(
        '{"code":"DOWNLOAD_FAILED","context":{"status":"404"}}',
      ),
    ).toBe("skills.repo.failureReason.notFound");
    expect(
      getSkillRepoFailureReasonKey(
        '{"code":"DOWNLOAD_FAILED","context":{"status":"429"}}',
      ),
    ).toBe("skills.repo.failureReason.accessLimited");
    expect(getSkillRepoFailureReasonKey("error sending request")).toBe(
      "skills.repo.failureReason.network",
    );
  });

  it("recognizes raw request timeouts before generic network failures", () => {
    expect(
      getSkillRepoFailureReasonKey(
        "error sending request for url: operation timed out",
      ),
    ).toBe("skills.repo.failureReason.timeout");
  });

  it("recognizes localized repository download timeout messages", () => {
    expect(
      getSkillRepoFailureReasonKey(
        "仓库下载地址超时（65秒）: https://codeload.github.com/owner/repo/zip/main",
      ),
    ).toBe("skills.repo.failureReason.timeout");
  });

  it("reports a repository without SKILL.md as no skills instead of an invalid archive", () => {
    expect(
      getSkillRepoFailureReasonKey('{"code":"NO_SKILLS_IN_ZIP","context":{}}'),
    ).toBe("skills.repo.failureReason.noSkills");
  });
});
