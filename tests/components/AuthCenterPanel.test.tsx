import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AuthCenterPanel } from "@/components/settings/AuthCenterPanel";
import userEvent from "@testing-library/user-event";
import {
  AUTH_CENTER_APPS,
  readAuthCenterPreferences,
  updateAuthCenterPreferences,
} from "@/lib/authCenterPreferences";
import type { Settings } from "@/types";
import { APP_IDS } from "@/config/appSpecs";

const mocks = vi.hoisted(() => ({
  settings: undefined as Partial<Settings> | undefined,
  canSwitchAgy: true,
}));

vi.mock("@/lib/query", () => ({
  useSettingsQuery: () => ({ data: mocks.settings }),
}));
vi.mock("@/lib/authBinding", () => ({
  canSwitchAgyAccount: () => mocks.canSwitchAgy,
}));

vi.mock("@/components/providers/forms/ClaudeOfficialSection", () => ({
  ClaudeOfficialSection: ({ helpSide }: { helpSide?: string }) => (
    <div data-testid="claude" data-help-side={helpSide} />
  ),
}));
vi.mock("@/components/providers/forms/CopilotAuthSection", () => ({
  CopilotAuthSection: ({ helpSide }: { helpSide?: string }) => (
    <div data-testid="copilot" data-help-side={helpSide} />
  ),
}));
vi.mock("@/components/providers/forms/CodexOAuthSection", () => ({
  CodexOAuthSection: ({ helpSide }: { helpSide?: string }) => (
    <div data-testid="codex" data-help-side={helpSide} />
  ),
}));
vi.mock("@/components/providers/forms/XaiOAuthSection", () => ({
  XaiOAuthSection: ({ helpSide }: { helpSide?: string }) => (
    <div data-testid="xai" data-help-side={helpSide} />
  ),
}));
vi.mock("@/components/providers/forms/AgyOAuthSection", () => ({
  AgyOAuthSection: ({ helpSide }: { helpSide?: string }) => (
    <div data-testid="agy" data-help-side={helpSide} />
  ),
}));
vi.mock("@/components/providers/KimiAccountLogin", () => ({
  KimiAccountLogin: ({ helpSide }: { helpSide?: string }) => (
    <div data-testid="kimi" data-help-side={helpSide} />
  ),
}));
vi.mock("@/components/settings/auth/WorkBuddyAccountSection", () => ({
  WorkBuddyAccountSection: ({ helpSide }: { helpSide?: string }) => (
    <div data-testid="workbuddy" data-help-side={helpSide} />
  ),
}));
vi.mock("@/components/settings/auth/AutoQuotaToggle", () => ({
  AutoQuotaToggle: () => null,
}));
vi.mock("@/components/providers/forms/CodebuddyAccountSection", () => ({
  CodebuddyAccountSection: ({ helpSide }: { helpSide?: string }) => (
    <div data-testid="codebuddy" data-help-side={helpSide} />
  ),
}));
vi.mock("@/components/providers/DshAccount", () => ({
  DshAccountLogin: ({ helpSide }: { helpSide?: string }) => (
    <div data-testid="dsh" data-help-side={helpSide} />
  ),
}));

beforeEach(() => {
  localStorage.removeItem("cc-switch.authCenter.apps");
});
afterEach(() => {
  localStorage.removeItem("cc-switch.authCenter.apps");
});

let resizeCallbacks: Array<() => void> = [];

const rect = (top: number) => ({ top }) as DOMRect;

/** 正文 overflow-y:auto，外层 #content-area overflow:hidden，和 FullScreenPanel 一致 */
const renderInPanel = () => {
  const outer = document.createElement("main");
  outer.style.overflow = "hidden";
  const container = document.createElement("div");
  container.style.overflowY = "auto";
  outer.appendChild(container);
  document.body.appendChild(outer);

  Object.defineProperty(container, "clientHeight", { value: 400 });
  Object.defineProperty(container, "scrollHeight", { value: 2000 });
  container.getBoundingClientRect = () => rect(100);

  const result = render(<AuthCenterPanel authScrollTarget="codex_oauth" />, {
    container,
  });
  return { ...result, outer, container };
};

/** 让 codex 卡片在文档里的位置是 offset，屏幕位置随正文滚动变化 */
const placeCodexAt = (container: HTMLElement, offset: () => number) => {
  const codex = container.querySelector('[data-testid="codex"]')!
    .parentElement as HTMLElement;
  codex.getBoundingClientRect = () =>
    rect(100 + offset() - container.scrollTop);
};

/** 卡片位置摆好后重新定位一次（首次挂载时 jsdom 还量不出位置） */
const retarget = (rerender: (ui: React.ReactElement) => void) => {
  rerender(<AuthCenterPanel authScrollTarget={null} />);
  resizeCallbacks = [];
  rerender(<AuthCenterPanel authScrollTarget="codex_oauth" />);
};

describe("AuthCenterPanel scroll target", () => {
  beforeEach(() => {
    resizeCallbacks = [];
    vi.stubGlobal(
      "ResizeObserver",
      class {
        constructor(callback: () => void) {
          resizeCallbacks.push(callback);
        }
        observe() {}
        disconnect() {}
      },
    );
    vi.spyOn(window, "requestAnimationFrame").mockImplementation((cb) => {
      cb(0);
      return 0;
    });
    Element.prototype.scrollIntoView = vi.fn();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
    document.body.innerHTML = "";
  });

  it("scrolls only the panel body, never the outer content area", () => {
    let offset = 600;
    const { outer, container, rerender } = renderInPanel();
    placeCodexAt(container, () => offset);
    retarget(rerender);

    expect(container.scrollTop).toBe(584);
    expect(outer.scrollTop).toBe(0);
    expect(outer.scrollLeft).toBe(0);
    expect(Element.prototype.scrollIntoView).not.toHaveBeenCalled();

    // 上面几组加载完把目标挤下去：跟着重新对齐
    offset = 800;
    resizeCallbacks.forEach((callback) => callback());
    expect(container.scrollTop).toBe(784);
  });

  it("stops re-aligning once the user scrolls", () => {
    let offset = 600;
    const { container, rerender } = renderInPanel();
    placeCodexAt(container, () => offset);
    retarget(rerender);
    expect(container.scrollTop).toBe(584);

    fireEvent.wheel(container);
    container.scrollTop = 300;
    offset = 800;
    resizeCallbacks.forEach((callback) => callback());
    expect(container.scrollTop).toBe(300);
  });

  it("disables scroll anchoring while pinning and restores it afterwards", () => {
    const { container, rerender, unmount } = renderInPanel();
    placeCodexAt(container, () => 600);
    retarget(rerender);
    // 浏览器滚动锚定会自己改 scrollTop，被误判成用户接管
    expect(container.style.overflowAnchor).toBe("none");

    fireEvent.wheel(container);
    expect(container.style.overflowAnchor).toBe("");

    retarget(rerender);
    expect(container.style.overflowAnchor).toBe("none");
    unmount();
    expect(container.style.overflowAnchor).toBe("");
  });
});

const CARD_TEST_ID =
  /^(claude|copilot|codex|xai|kimi|dsh|agy|workbuddy|codebuddy)$/;

const renderedCards = () => screen.queryAllByTestId(CARD_TEST_ID);

const cardOrder = () => renderedCards().map((card) => card.dataset.testid);

const cardHelpSides = () =>
  renderedCards().map((card) => [card.dataset.testid, card.dataset.helpSide]);

const onlyVisibleApps = (
  ...shown: (typeof APP_IDS)[number][]
): Settings["visibleApps"] => {
  const shownSet = new Set(shown);
  return Object.fromEntries(
    APP_IDS.map((app) => [app, shownSet.has(app)]),
  ) as Settings["visibleApps"];
};

describe("AuthCenterPanel card order and visibility", () => {
  afterEach(() => {
    mocks.settings = undefined;
    mocks.canSwitchAgy = true;
  });

  it("keeps the default order before the sidebar is customized", () => {
    render(<AuthCenterPanel />);
    expect(cardOrder()).toEqual([
      "claude",
      "copilot",
      "codex",
      "xai",
      "kimi",
      "dsh",
      "agy",
      "workbuddy",
      "codebuddy",
    ]);
  });

  it("follows the order dragged in the Apps page", () => {
    // 侧栏里 Antigravity、WorkBuddy 拖到最前，Claude Code 拖到最后
    const moved = new Set(["agy", "workbuddy", "claude"]);
    mocks.settings = {
      appOrder: [
        "agy",
        "workbuddy",
        ...APP_IDS.filter((app) => !moved.has(app)),
        "claude",
      ],
    };
    render(<AuthCenterPanel />);
    // Copilot、ChatGPT、xAI 跟着 Claude Desktop 走；Claude 官方只看 Claude Code
    expect(cardOrder()).toEqual([
      "agy",
      "workbuddy",
      "copilot",
      "codex",
      "xai",
      "kimi",
      "dsh",
      "codebuddy",
      "claude",
    ]);
  });

  it("hides a card only when every app using that account is hidden", () => {
    mocks.settings = {
      visibleApps: {
        claude: false,
        "claude-desktop": false,
        codex: false,
        grokbuild: false,
        kimi: false,
      } as Settings["visibleApps"],
    };
    render(<AuthCenterPanel />);
    // Copilot CLI 还显示着：Copilot 卡留下，排到 Copilot CLI 的位置；
    // ChatGPT、xAI（含 Grok Build）、Claude 官方、Kimi 的应用全藏了
    expect(cardOrder()).toEqual([
      "dsh",
      "agy",
      "copilot",
      "workbuddy",
      "codebuddy",
    ]);
  });

  it("keeps the xAI card visible when Grok Build is shown", () => {
    mocks.settings = {
      visibleApps: {
        claude: false,
        "claude-desktop": false,
        codex: false,
      } as Settings["visibleApps"],
    };
    const { rerender } = render(<AuthCenterPanel />);
    expect(cardOrder()).toEqual([
      "xai",
      "kimi",
      "dsh",
      "agy",
      "copilot",
      "workbuddy",
      "codebuddy",
    ]);

    // 只保留 Grok Build 一个应用时，xAI 卡仍然可见且只有这一张
    mocks.settings = {
      visibleApps: onlyVisibleApps("grokbuild"),
    };
    rerender(<AuthCenterPanel />);
    expect(cardHelpSides()).toEqual([["xai", "bottom"]]);
  });

  it("moves the xAI card to the top when Grok Build is dragged first", () => {
    mocks.settings = {
      appOrder: ["grokbuild", ...APP_IDS.filter((app) => app !== "grokbuild")],
    };
    render(<AuthCenterPanel />);
    expect(cardOrder()[0]).toBe("xai");
  });

  it("sets helpSide to top only for the last card when multiple cards render", () => {
    const { rerender } = render(<AuthCenterPanel />);
    expect(cardHelpSides()).toEqual([
      ["claude", "bottom"],
      ["copilot", "bottom"],
      ["codex", "bottom"],
      ["xai", "bottom"],
      ["kimi", "bottom"],
      ["dsh", "bottom"],
      ["agy", "bottom"],
      ["workbuddy", "bottom"],
      ["codebuddy", "top"],
    ]);

    // 拖拽排序后：WorkBuddy 排在最前改为向下弹，Claude 排在最后改为向上弹
    const moved = new Set(["workbuddy", "claude"]);
    mocks.settings = {
      appOrder: [
        "workbuddy",
        ...APP_IDS.filter((app) => !moved.has(app)),
        "claude",
      ],
    };
    rerender(<AuthCenterPanel />);
    expect(cardHelpSides()[0]).toEqual(["workbuddy", "bottom"]);
    expect(cardHelpSides().at(-1)).toEqual(["claude", "top"]);

    // GitHub Copilot 成为多卡里的最后一张时同样向上弹
    mocks.settings = {
      visibleApps: onlyVisibleApps("dsh", "copilot"),
    };
    rerender(<AuthCenterPanel />);
    expect(cardHelpSides()).toEqual([
      ["dsh", "bottom"],
      ["copilot", "top"],
    ]);

    // ChatGPT（codex_oauth）成为多卡里的最后一张时同样向上弹
    mocks.settings = {
      appOrder: [
        "copilot",
        "grokbuild",
        "codex",
        ...APP_IDS.filter(
          (app) => app !== "copilot" && app !== "grokbuild" && app !== "codex",
        ),
      ],
      visibleApps: onlyVisibleApps("copilot", "grokbuild", "codex"),
    };
    rerender(<AuthCenterPanel />);
    expect(cardHelpSides()).toEqual([
      ["copilot", "bottom"],
      ["xai", "bottom"],
      ["codex", "top"],
    ]);

    // 只剩一张卡片（即使是原本垫底的 WorkBuddy）时贴近顶部，向下弹
    mocks.settings = {
      visibleApps: onlyVisibleApps("workbuddy"),
    };
    rerender(<AuthCenterPanel />);
    expect(cardHelpSides()).toEqual([["workbuddy", "bottom"]]);

    // 所有应用都隐藏时不渲染任何卡片
    mocks.settings = {
      visibleApps: onlyVisibleApps(),
    };
    rerender(<AuthCenterPanel />);
    expect(cardOrder()).toEqual([]);
  });

  it("always shows the card a provider form jumps to", () => {
    mocks.settings = {
      visibleApps: { kimi: false } as Settings["visibleApps"],
    };
    const { rerender } = render(
      <AuthCenterPanel authScrollTarget="kimi_oauth" />,
    );
    expect(cardOrder()).toContain("kimi");

    // 全部应用都隐藏时，从表单跳转进来的单张卡片依然渲染且说明卡向下弹
    mocks.settings = {
      visibleApps: onlyVisibleApps(),
    };
    rerender(<AuthCenterPanel authScrollTarget="xai_oauth" />);
    expect(cardHelpSides()).toEqual([["xai", "bottom"]]);
  });

  it("omits Antigravity where account switching is unsupported", () => {
    mocks.canSwitchAgy = false;
    const { rerender } = render(<AuthCenterPanel />);
    expect(cardOrder()).not.toContain("agy");

    // 过滤掉不可用的 agy 后只剩 1 张卡片，说明卡按实际渲染数量改为向下弹
    mocks.settings = {
      visibleApps: onlyVisibleApps("agy", "workbuddy"),
    };
    rerender(<AuthCenterPanel />);
    expect(cardHelpSides()).toEqual([["workbuddy", "bottom"]]);
  });
});

describe("AuthCenterPanel app preferences", () => {
  beforeEach(() => {
    mocks.canSwitchAgy = false;
  });
  const sectionOrder = () =>
    screen
      .getAllByTestId(/^(claude|copilot|codex|xai|kimi|dsh|agy|workbuddy)$/)
      .map((element) => element.dataset.testid);

  it("reorders cards and remembers the order after reopening the panel", async () => {
    const user = userEvent.setup();
    const { unmount } = render(<AuthCenterPanel />);
    expect(sectionOrder().slice(0, 3)).toEqual(["claude", "copilot", "codex"]);
    await user.click(screen.getByRole("button", { name: "管理应用" }));
    expect(
      screen.getByRole("button", { name: "Claude 上移一位" }),
    ).toBeDisabled();
    await user.click(screen.getByRole("button", { name: "ChatGPT 上移一位" }));
    expect(sectionOrder().slice(0, 3)).toEqual(["claude", "codex", "copilot"]);
    unmount();

    render(<AuthCenterPanel />);
    expect(sectionOrder().slice(0, 3)).toEqual(["claude", "codex", "copilot"]);
  });

  it("sorts through the drag handle using the keyboard and supports cancelling", async () => {
    const user = userEvent.setup();
    render(<AuthCenterPanel />);
    await user.click(screen.getByRole("button", { name: "管理应用" }));
    const handles = screen.getAllByRole("button", { name: /^拖动调整/ });
    for (const handle of handles) {
      const row = handle.parentElement!;
      row.getBoundingClientRect = () => {
        const top = Array.from(row.parentElement!.children).indexOf(row) * 40;
        return {
          x: 0,
          y: top,
          left: 0,
          right: 400,
          top,
          bottom: top + 40,
          width: 400,
          height: 40,
          toJSON: () => ({}),
        };
      };
    }
    handles[0].focus();
    await user.keyboard("[Space][ArrowDown][Space]");
    expect(sectionOrder().slice(0, 2)).toEqual(["copilot", "claude"]);
    expect(readAuthCenterPreferences().order.slice(0, 2)).toEqual([
      "github_copilot",
      "claude_official",
    ]);

    handles[0].focus();
    await user.keyboard("[Space][ArrowUp][Escape]");
    expect(sectionOrder().slice(0, 2)).toEqual(["copilot", "claude"]);
  });

  it("hides apps, keeps their switches available, and can restore defaults", async () => {
    const user = userEvent.setup();
    const { unmount } = render(<AuthCenterPanel />);
    await user.click(screen.getByRole("button", { name: "管理应用" }));
    await user.click(screen.getByRole("switch", { name: "显示 Claude" }));
    expect(screen.queryByTestId("claude")).not.toBeInTheDocument();
    expect(
      screen.getByRole("switch", { name: "显示 Claude" }),
    ).not.toBeChecked();
    unmount();

    render(<AuthCenterPanel />);
    expect(screen.queryByTestId("claude")).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "管理应用" }));
    await user.click(screen.getByRole("switch", { name: "显示 Claude" }));
    expect(screen.getByTestId("claude")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Claude 下移一位" }));
    await user.click(screen.getByRole("switch", { name: "显示 ChatGPT" }));
    await user.click(screen.getByRole("button", { name: "恢复默认" }));
    expect(sectionOrder().slice(0, 3)).toEqual(["claude", "copilot", "codex"]);
    expect(screen.getByRole("switch", { name: "显示 ChatGPT" })).toBeChecked();
  });

  it("allows hiding every app and still offers a way to enable them", async () => {
    act(() =>
      updateAuthCenterPreferences((current) => ({
        ...current,
        hidden: AUTH_CENTER_APPS.map(({ id }) => id),
      })),
    );
    const user = userEvent.setup();
    render(<AuthCenterPanel />);
    expect(
      screen.getByText("所有应用已隐藏，点击「管理应用」重新开启。"),
    ).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "管理应用" }));
    await user.click(screen.getByRole("switch", { name: "显示 ChatGPT" }));
    expect(screen.getByTestId("codex")).toBeInTheDocument();
  });

  it("temporarily reveals a hidden navigation target without changing saved preferences", async () => {
    updateAuthCenterPreferences((current) => ({
      ...current,
      hidden: ["codex_oauth"],
    }));
    const { rerender } = render(<AuthCenterPanel />);
    expect(screen.queryByTestId("codex")).not.toBeInTheDocument();
    rerender(<AuthCenterPanel authScrollTarget="codex_oauth" />);
    expect(screen.getByTestId("codex")).toBeInTheDocument();
    expect(
      screen.getByText("已临时显示 ChatGPT，可在「管理应用」中重新开启。"),
    ).toBeInTheDocument();
    expect(readAuthCenterPreferences().hidden).toContain("codex_oauth");

    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "保持显示" }));
    expect(readAuthCenterPreferences().hidden).not.toContain("codex_oauth");
  });

  it("can close a temporarily revealed target and hides it again when leaving the target", async () => {
    updateAuthCenterPreferences((current) => ({
      ...current,
      hidden: ["codex_oauth"],
    }));
    const user = userEvent.setup();
    const { rerender } = render(
      <AuthCenterPanel authScrollTarget="codex_oauth" />,
    );
    expect(screen.getByTestId("codex")).toBeInTheDocument();
    rerender(<AuthCenterPanel authScrollTarget={null} />);
    expect(screen.queryByTestId("codex")).not.toBeInTheDocument();
    rerender(<AuthCenterPanel authScrollTarget="codex_oauth" />);
    await user.click(screen.getByRole("button", { name: "管理应用" }));
    expect(screen.getByRole("switch", { name: "显示 ChatGPT" })).toBeChecked();
    await user.click(screen.getByRole("switch", { name: "显示 ChatGPT" }));
    expect(screen.queryByTestId("codex")).not.toBeInTheDocument();
  });

  it("only lists Antigravity on supported platforms and preserves its saved position", async () => {
    const user = userEvent.setup();
    const { unmount } = render(<AuthCenterPanel />);
    await user.click(screen.getByRole("button", { name: "管理应用" }));
    expect(
      screen.queryByRole("switch", { name: "显示 Antigravity" }),
    ).not.toBeInTheDocument();
    unmount();
    mocks.canSwitchAgy = true;
    render(<AuthCenterPanel />);
    await user.click(screen.getByRole("button", { name: "管理应用" }));
    expect(screen.getByTestId("agy")).toBeInTheDocument();
    expect(
      screen.getByRole("switch", { name: "显示 Antigravity" }),
    ).toBeChecked();
    expect(sectionOrder().slice(-2)).toEqual(["agy", "workbuddy"]);
  });

  it("can show an app independently of its sidebar visibility", async () => {
    mocks.settings = {
      visibleApps: { kimi: false } as Settings["visibleApps"],
    };
    const user = userEvent.setup();
    render(<AuthCenterPanel />);
    expect(screen.queryByTestId("kimi")).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "管理应用" }));
    const toggle = screen.getByRole("switch", { name: "显示 Kimi Code" });
    expect(toggle).not.toBeChecked();
    await user.click(toggle);
    expect(screen.getByTestId("kimi")).toBeInTheDocument();
    expect(mocks.settings.visibleApps?.kimi).toBe(false);
    await user.click(screen.getByRole("button", { name: "恢复默认" }));
    expect(screen.queryByTestId("kimi")).not.toBeInTheDocument();
  });

  it("includes the newly added CodeBuddy app in visibility controls", async () => {
    const user = userEvent.setup();
    render(<AuthCenterPanel />);
    expect(screen.getByTestId("codebuddy")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "管理应用" }));
    await user.click(screen.getByRole("switch", { name: "显示 CodeBuddy" }));
    expect(screen.queryByTestId("codebuddy")).not.toBeInTheDocument();
    await user.click(screen.getByRole("switch", { name: "显示 CodeBuddy" }));
    expect(screen.getByTestId("codebuddy")).toBeInTheDocument();
  });
});
