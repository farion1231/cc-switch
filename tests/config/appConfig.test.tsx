import { describe, expect, it } from "vitest";
import {
  ADDITIVE_APP_IDS,
  APP_IDS,
  isAdditiveAppId,
} from "@/config/appConfig";

describe("custom slim app set", () => {
  it("exposes only Claude Code, Claude Desktop, and Codex", () => {
    expect(APP_IDS).toEqual(["claude", "claude-desktop", "codex"]);
  });

  it("does not expose additive upstream apps", () => {
    expect(ADDITIVE_APP_IDS).toEqual([]);
    for (const appId of ["opencode", "openclaw", "hermes", "pi", "mcode"]) {
      expect(isAdditiveAppId(appId)).toBe(false);
    }
  });
});
