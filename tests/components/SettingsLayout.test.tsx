import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { SettingsRow } from "@/components/settings/SettingsLayout";

describe("SettingsRow", () => {
  it("keeps the summary visible under the label and the details behind the ?", () => {
    render(
      <SettingsRow
        label="记录请求用量"
        summary="关闭后，用量统计里没有经路由的请求明细。"
        help={{ title: "记录请求用量", body: "规则说明" }}
      />,
    );

    // 后果常驻可见，不藏进「?」
    expect(
      screen.getByText("关闭后，用量统计里没有经路由的请求明细。"),
    ).not.toHaveClass("sr-only");
    // 规则说明只挂在「?」上（读屏念作按钮的描述，视觉上收起）
    expect(
      screen.getByRole("button", { name: "记录请求用量" }),
    ).toHaveAccessibleDescription("规则说明");
    expect(screen.getByText("规则说明")).toHaveClass("sr-only");
  });

  it("renders neither summary nor ? when none is given", () => {
    render(<SettingsRow label="界面语言" />);

    expect(screen.getByText("界面语言")).toBeInTheDocument();
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
  });
});
