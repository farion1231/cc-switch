import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { AppSwitcher } from "@/components/AppSwitcher";
import { DEFAULT_VISIBLE_APPS } from "@/config/appConfig";
import type { VisibleApps } from "@/types";

const allVisible = DEFAULT_VISIBLE_APPS;

/** 测试环境 i18n 资源为空，t() 回落成键名本身；× 的 title 即 "appSwitcher.hide" */
const hideButtonSelector = "button[title='appSwitcher.hide']";

describe("AppSwitcher", () => {
  it("点 × 隐藏对应应用，不触发切换", () => {
    const onSwitch = vi.fn();
    const onHideApp = vi.fn();
    render(
      <AppSwitcher
        activeApp="claude"
        onSwitch={onSwitch}
        visibleApps={allVisible}
        onHideApp={onHideApp}
      />,
    );
    const piTab = screen.getByRole("button", { name: "Pi" });
    const hidePi = piTab.parentElement!.querySelector(hideButtonSelector);
    expect(hidePi, "Pi 的 tab 上应有 ×").not.toBeNull();
    fireEvent.click(hidePi!);
    expect(onHideApp).toHaveBeenCalledTimes(1);
    expect(onHideApp).toHaveBeenCalledWith("pi");
    expect(onSwitch).not.toHaveBeenCalled();
  });

  it("只剩一个可见应用时不再出 ×（与设置页同一护栏）", () => {
    const onlyClaude: VisibleApps = {
      claude: true,
      "claude-desktop": false,
      codex: false,
      gemini: false,
      grokbuild: false,
      opencode: false,
      openclaw: false,
      hermes: false,
      pi: false,
    };
    const { container } = render(
      <AppSwitcher
        activeApp="claude"
        onSwitch={vi.fn()}
        visibleApps={onlyClaude}
        onHideApp={vi.fn()}
      />,
    );
    expect(container.querySelector(hideButtonSelector)).toBeNull();
    expect(
      screen.getByRole("button", { name: "Claude Code" }),
    ).toBeInTheDocument();
  });

  it("未传 onHideApp 时不渲染 ×，未传 onShowApp 时不渲染「+」", () => {
    const { container } = render(
      <AppSwitcher
        activeApp="claude"
        onSwitch={vi.fn()}
        visibleApps={allVisible}
      />,
    );
    expect(container.querySelector(hideButtonSelector)).toBeNull();
    expect(screen.queryByTitle("appSwitcher.add")).not.toBeInTheDocument();
  });

  it("「+」浮层列出隐藏应用，点击加回", () => {
    const onShowApp = vi.fn();
    const partlyHidden: VisibleApps = { ...allVisible, pi: false };
    render(
      <AppSwitcher
        activeApp="claude"
        onSwitch={vi.fn()}
        visibleApps={partlyHidden}
        onShowApp={onShowApp}
      />,
    );
    expect(
      screen.queryByRole("button", { name: /^Pi$/ }),
      "隐藏的 Pi 不应出现在 tab 里",
    ).not.toBeInTheDocument();
    fireEvent.click(screen.getByTitle("appSwitcher.add"));
    // ProviderIcon 给部分品牌图标带 alt 文本，条目按钮的可访问名是拼接值，用正则
    fireEvent.click(screen.getByRole("button", { name: /Pi/ }));
    expect(onShowApp).toHaveBeenCalledWith("pi");
  });

  it("「+」常驻；全部可见时点开是空态", () => {
    render(
      <AppSwitcher
        activeApp="claude"
        onSwitch={vi.fn()}
        visibleApps={allVisible}
        onShowApp={vi.fn()}
      />,
    );
    fireEvent.click(screen.getByTitle("appSwitcher.add"));
    expect(screen.getByText("appSwitcher.allShown")).toBeInTheDocument();
  });
});
