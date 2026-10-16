import { describe, expect, it } from "vitest";
import { PROMPT_APP_IDS, promptAppOf } from "@/lib/query/prompts";
import { APP_IDS } from "@/config/appConfig";

/**
 * 回归：后端 `prompt_files::prompt_file_path` 对 Cursor 有完整支持
 * （`AppType::Cursor => ~/.cursor/AGENTS.md`），但前端 `PROMPT_APP_IDS` 漏了它 ——
 * 结果提示词页的应用下拉里没有 Cursor，Cursor 应用页页头也没有「提示词 ›」入口。
 */
describe("PROMPT_APP_IDS", () => {
  it("包含 cursor（后端支持 ~/.cursor/AGENTS.md）", () => {
    expect(PROMPT_APP_IDS).toContain("cursor");
  });

  it("按侧栏顺序排列（与 APP_IDS 的相对次序一致）", () => {
    const inSidebarOrder = APP_IDS.filter((app) => PROMPT_APP_IDS.includes(app));
    expect(PROMPT_APP_IDS).toEqual(inSidebarOrder);
  });

  it("promptAppOf 把 cursor 解析成它自己", () => {
    expect(promptAppOf("cursor")).toBe("cursor");
  });
});
