import { describe, expect, it } from "vitest";

import {
  DEFAULT_VISIBLE_APPS,
  EMPTY_VISIBLE_APPS,
  filterVisibleAppIds,
  getSkillTargetApp,
  resolveVisibleApps,
} from "@/config/appConfig";

describe("appConfig visibility helpers", () => {
  it("keeps Hermes hidden by default and hides every app before settings resolve", () => {
    expect(DEFAULT_VISIBLE_APPS.hermes).toBe(false);
    expect(Object.values(EMPTY_VISIBLE_APPS).every((visible) => !visible)).toBe(
      true,
    );
    expect(resolveVisibleApps().hermes).toBe(false);
  });

  it("filters app ids using persisted defaults after settings load", () => {
    expect(filterVisibleAppIds(["claude", "hermes"], undefined)).toEqual([
      "claude",
    ]);
    expect(
      filterVisibleAppIds(["claude", "hermes"], EMPTY_VISIBLE_APPS),
    ).toEqual([]);
  });

  it("selects a visible app for skill install flows", () => {
    const visibleApps = {
      claude: false,
      "claude-desktop": false,
      codex: true,
      gemini: true,
      opencode: true,
      openclaw: true,
      hermes: false,
    };

    expect(getSkillTargetApp("openclaw", visibleApps)).toBe("codex");
    expect(getSkillTargetApp("claude", visibleApps)).toBe("codex");
    expect(getSkillTargetApp("gemini", visibleApps)).toBe("gemini");
    expect(
      getSkillTargetApp("openclaw", {
        claude: false,
        "claude-desktop": false,
        codex: false,
        gemini: false,
        opencode: false,
        openclaw: true,
        hermes: false,
      }),
    ).toBeNull();
  });
});
