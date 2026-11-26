import { describe, expect, it } from "vitest";
import { isClaudeDesktopNeedsRoutingError } from "@/utils/errorUtils";

describe("isClaudeDesktopNeedsRoutingError", () => {
  it("recognizes the localized backend guard", () => {
    expect(
      isClaudeDesktopNeedsRoutingError(
        "这个供应商要经本地路由才能用（模型映射或托管账号），请先把 Claude Desktop 切到路由模式 (This provider only works through local routing (model mapping or a managed account); switch Claude Desktop to the routing mode first)",
      ),
    ).toBe(true);
  });

  it("does not classify unrelated provider errors", () => {
    expect(
      isClaudeDesktopNeedsRoutingError("provider credentials missing"),
    ).toBe(false);
  });
});
