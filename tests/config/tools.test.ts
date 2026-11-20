import { expect, it } from "vitest";
import {
  TOOL_DISPLAY_NAMES,
  TOOL_NAMES,
} from "@/components/apps/useToolManagement";
import backendTools from "./tools.json";

// tools.json 由后端测试 tools_match_shared_fixture 从 TOOLS 生成：
// 前端管理的工具和后端一一对应，提示里用的名字也一样。
it("manages the same tools under the same names as the backend", () => {
  expect(
    Object.fromEntries(
      TOOL_NAMES.map((tool) => [tool, TOOL_DISPLAY_NAMES[tool]]),
    ),
  ).toEqual(backendTools);
});
