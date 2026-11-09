import { screen } from "@testing-library/react";
import { renderWithQueryClient as render } from "../../../utils/testQueryClient";
import { describe, expect, it } from "vitest";
import { getAgentReaderStyle } from "@/components/sessions/reader/agentStyles";
import { transcriptToMarkdown } from "@/components/sessions/reader/exportMarkdown";
import { StepMetaText } from "@/components/sessions/reader/rowStyles";
import { buildTurns } from "@/components/sessions/reader/turns";
import { formatMoneyCompact, USD_DISPLAY } from "@/components/usage/format";
import { fixtures } from "./helpers";

// 会话阅读页的费用只来自 OpenCode 的 step-finish，本机常常没有这类数据，
// 这里把「界面显示」和「导出 Markdown」两条出口各钉一次。
describe("session reader step cost", () => {
  it("renders the step cost through the shared money formatter", () => {
    render(
      <StepMetaText
        meta={{
          status: "success",
          glyph: "●",
          tone: "success",
          tokens: 28_500,
          costUsd: 0.0123,
        }}
      />,
    );
    expect(screen.getByText("$0.012")).toBeInTheDocument();
  });

  it("writes step costs into the exported markdown with the given formatter", () => {
    const style = getAgentReaderStyle("opencode");
    const markdown = transcriptToMarkdown(
      "OpenCode",
      buildTurns(fixtures.opencode, { style }),
      {
        style,
        appName: "OpenCode",
        youLabel: "You",
        t: (key) => key,
        formatCost: (usd) => formatMoneyCompact(usd, USD_DISPLAY),
      },
    );
    expect(markdown).toContain("$0.0081");
  });
});
