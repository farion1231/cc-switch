import { describe, expect, it } from "vitest";
import {
  APP_IDS,
  orderAppIds,
  isAdditiveAppId,
  SKILLS_APP_IDS,
  MCP_APP_IDS,
  isProxyAppId,
} from "@/config/appConfig";

describe("appConfig provider lifecycle", () => {
  it.each(["opencode", "openclaw", "hermes", "pi", "mcode", "kimi"])(
    "classifies %s as additive",
    (appId) => {
      expect(isAdditiveAppId(appId)).toBe(true);
    },
  );

  it.each(["claude", "claude-desktop", "codex", "gemini", "grokbuild"])(
    "does not classify %s as additive",
    (appId) => {
      expect(isAdditiveAppId(appId)).toBe(false);
    },
  );
});

// Antigravity shares management UI, but has independent configuration and no CCS proxy.
describe("Antigravity capabilities", () => {
  it("supports skills and MCP with direct provider switching", () => {
    expect(SKILLS_APP_IDS).toContain("agy");
    expect(MCP_APP_IDS).toContain("agy");
    expect(isAdditiveAppId("agy")).toBe(false);
    expect(isProxyAppId("agy")).toBe(false);
  });
});

describe("orderAppIds", () => {
  it("falls back to the default order when nothing is saved", () => {
    expect(orderAppIds(undefined)).toEqual(APP_IDS);
    expect(orderAppIds([])).toEqual(APP_IDS);
  });

  it("keeps the saved order and drops unknown or repeated ids", () => {
    const saved = [...APP_IDS].reverse();
    expect(orderAppIds(["future-app", ...saved, "claude"])).toEqual(saved);
  });

  it("puts an app missing from the saved order after its default predecessor", () => {
    const saved = APP_IDS.filter((app) => app !== "kimi");
    const [first, ...rest] = saved;
    // 把 mcode（kimi 默认的前一个）挪到最后，kimi 应跟着它
    const moved = [first, ...rest.filter((app) => app !== "mcode"), "mcode"];
    expect(orderAppIds(moved)).toEqual([...moved, "kimi"]);
    expect(orderAppIds(APP_IDS.filter((app) => app !== "claude"))[0]).toBe(
      "claude",
    );
  });
});
