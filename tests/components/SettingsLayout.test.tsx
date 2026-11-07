import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { SettingsRow } from "@/components/settings/SettingsLayout";

describe("SettingsRow", () => {
  it("shows the summary under the label and keeps the details behind the ?", () => {
    render(
      <SettingsRow
        label="统一 Codex 会话历史"
        summary="官方和第三方会话显示在同一列表"
        help={{
          title: "统一 Codex 会话历史",
          body: "可选择把现有官方会话一并迁入",
        }}
      />,
    );

    // 摘要常驻可见
    expect(
      screen.getByText("官方和第三方会话显示在同一列表"),
    ).toBeInTheDocument();
    // 规则说明不常驻，只挂在「?」上（读屏念作按钮的描述，视觉上收起）
    const helpButton = screen.getByRole("button", {
      name: "统一 Codex 会话历史",
    });
    expect(helpButton).toHaveAccessibleDescription(
      "可选择把现有官方会话一并迁入",
    );
    expect(screen.getByText("可选择把现有官方会话一并迁入")).toHaveClass(
      "sr-only",
    );
  });

  it("renders neither summary nor ? for self-explanatory settings", () => {
    render(<SettingsRow label="界面语言" />);

    expect(screen.getByText("界面语言")).toBeInTheDocument();
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
  });
});
