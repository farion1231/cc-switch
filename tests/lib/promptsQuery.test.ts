import { describe, expect, it } from "vitest";
import { promptAppOf } from "@/lib/query/prompts";
import { APP_IDS, APP_IDS_WITH } from "@/config/appSpecs";
import type { AppId } from "@/lib/api/types";

/** 能力表是唯一来源；这里放宽到 AppId 便于按侧栏顺序比对与查表。 */
const PROMPT_APPS: readonly AppId[] = APP_IDS_WITH.prompts;

/**
 * 回归：提示词应用清单由能力表 `APP_IDS_WITH.prompts` 驱动，不再维护手写常量。
 * 后端 `prompt_files::prompt_file_path` 对 Cursor 有完整支持
 * （`AppType::Cursor => ~/.cursor/rules/cc-switch.mdc`），前端清单必须跟着能力表带上它。
 */
describe("prompt app list", () => {
  it("contains every prompts-capable app, including cursor", () => {
    expect(PROMPT_APPS).toContain("cursor");
  });

  it("keeps the sidebar order of APP_IDS", () => {
    const inSidebarOrder = APP_IDS.filter((app) => PROMPT_APPS.includes(app));
    expect(PROMPT_APPS).toEqual(inSidebarOrder);
  });

  it("promptAppOf follows APP_IDS_WITH.prompts for every app", () => {
    for (const app of APP_IDS) {
      const expected =
        app === "claude-desktop"
          ? "claude"
          : PROMPT_APPS.includes(app)
            ? app
            : null;
      expect(promptAppOf(app), app).toBe(expected);
    }
  });

  it("resolves cursor to itself", () => {
    expect(promptAppOf("cursor")).toBe("cursor");
  });
});
