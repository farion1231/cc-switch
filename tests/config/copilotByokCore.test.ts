import { describe, it, expect } from "vitest";
import en from "@/i18n/locales/en.json";
import zh from "@/i18n/locales/zh.json";
import zhTW from "@/i18n/locales/zh-TW.json";
import ja from "@/i18n/locales/ja.json";
import {
  APP_IDS,
  MCP_APP_IDS,
  SKILLS_APP_IDS,
  PROXY_APP_IDS,
} from "@/config/appConfig";
import { appPageBelongsTo } from "@/lib/navigation";

function keys(value: Record<string, unknown>, prefix = ""): string[] {
  return Object.entries(value)
    .flatMap(([key, item]) =>
      typeof item === "string"
        ? [`${prefix}${key}`]
        : keys(item as Record<string, unknown>, `${prefix}${key}.`),
    )
    .sort();
}

describe("BYOK core scope", () => {
  it("has matching keys in all four locales", () => {
    for (const locale of [zh, zhTW, ja])
      expect(keys(locale.copilotByok)).toEqual(keys(en.copilotByok));
  });
  it("registers only BYOK, without unsupported feature capabilities", () => {
    expect(APP_IDS).toContain("copilot-byok");
    expect(APP_IDS).not.toContain("copilot-cli");
    for (const list of [MCP_APP_IDS, SKILLS_APP_IDS, PROXY_APP_IDS])
      expect(list).not.toContain("copilot-byok");
    expect(appPageBelongsTo("copilotTargets", "copilot-byok")).toBe(true);
    expect(appPageBelongsTo("copilotTargets", "codex")).toBe(false);
  });
});
