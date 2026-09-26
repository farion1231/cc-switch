import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { PluginSettingsPanel } from "@/components/settings/PluginSettingsPanel";
import type { PluginInfo, PluginListResult } from "@/types/plugin";

const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }));

// 覆盖 setupTests.ts 里的全局 tauriMocks：本面板直接走 invoke 调 plugin_* 命令
vi.mock("@tauri-apps/api/core", () => ({
  invoke: invokeMock,
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, params?: Record<string, unknown>) =>
      params && "loaded" in params ? `${key}:${params.loaded}` : key,
  }),
}));

const builtinPlugin: PluginInfo = {
  id: "builtin:cache-injector",
  displayName: "Cache Injector",
  description: "Inject cache control breakpoints",
  isBuiltin: true,
  stages: ["pre_send"],
  priority: 200,
  enabled: true,
  version: null,
  source: null,
  hasConfig: false,
  error: null,
};

const userPlugin: PluginInfo = {
  id: "user:my-plugin",
  displayName: "My Plugin",
  description: "A user plugin",
  isBuiltin: false,
  stages: ["pre_request", "post_response"],
  priority: 500,
  enabled: false,
  version: "0.1.0",
  source: "~/.cc-switch/plugins/my-plugin/plugin.json",
  hasConfig: false,
  error: null,
};

const failedPlugin: PluginInfo = {
  id: "user:broken",
  displayName: "broken",
  description: "",
  isBuiltin: false,
  stages: [],
  priority: 500,
  enabled: false,
  version: null,
  source: "~/.cc-switch/plugins/broken/plugin.json",
  hasConfig: false,
  error: "插件清单无效: missing command",
};

const mockList = (plugins: PluginInfo[], globalEnabled = true) => {
  const result: PluginListResult = { plugins, globalEnabled };
  invokeMock.mockImplementation((command: string) => {
    switch (command) {
      case "plugin_list":
        return Promise.resolve(result);
      case "plugin_set_enabled":
      case "plugin_set_priority":
      case "plugin_set_all_enabled":
      case "plugin_open_dir":
        return Promise.resolve(undefined);
      case "plugin_reload":
        return Promise.resolve({ loaded: 2, errors: ["broken: bad manifest"] });
      default:
        return Promise.reject(new Error(`unexpected command: ${command}`));
    }
  });
};

beforeEach(() => {
  invokeMock.mockReset();
});

describe("PluginSettingsPanel", () => {
  it("renders plugin rows with badges, stages and error entries", async () => {
    mockList([builtinPlugin, userPlugin, failedPlugin]);

    render(<PluginSettingsPanel />);

    await waitFor(() => {
      expect(screen.getByText("Cache Injector")).toBeInTheDocument();
    });
    expect(screen.getByText("My Plugin")).toBeInTheDocument();
    expect(screen.getByText("broken")).toBeInTheDocument();
    // 徽章（i18n key 由 mock 直接回显）
    expect(screen.getByText("settings.advanced.plugins.badgeBuiltin")).toBeInTheDocument();
    expect(screen.getByText("settings.advanced.plugins.badgeUser")).toBeInTheDocument();
    // 加载失败条目置灰展示错误
    expect(screen.getByText("settings.advanced.plugins.loadFailed")).toBeInTheDocument();
    expect(screen.getByText("插件清单无效: missing command")).toBeInTheDocument();
    // stages 小标签按原样展示
    expect(screen.getByText("pre_send")).toBeInTheDocument();
    expect(screen.getByText("pre_request")).toBeInTheDocument();
    expect(screen.getByText("post_response")).toBeInTheDocument();
  });

  it("renders empty state when plugin_list returns nothing", async () => {
    mockList([]);
    render(<PluginSettingsPanel />);
    await waitFor(() => {
      expect(screen.getByText("settings.advanced.plugins.empty")).toBeInTheDocument();
    });
  });

  it("toggles a plugin via plugin_set_enabled", async () => {
    // 使用全部启用的列表，保证总开关为开、行内开关可用
    mockList([builtinPlugin]);
    render(<PluginSettingsPanel />);

    await waitFor(() => {
      expect(screen.getByText("Cache Injector")).toBeInTheDocument();
    });

    // 行内开关是页面最后一个 switch（全局总开关在 DOM 中排最前），
    // 按 aria-checked 匹配会误中总开关
    const rowSwitch = screen.getAllByRole("switch").at(-1);
    expect(rowSwitch).toBeDefined();

    await act(async () => {
      fireEvent.click(rowSwitch!);
    });

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("plugin_set_enabled", {
        id: "builtin:cache-injector",
        enabled: false,
      });
    });
  });

  it("commits priority on blur via plugin_set_priority", async () => {
    mockList([builtinPlugin]);
    render(<PluginSettingsPanel />);

    const priorityInput = await screen.findByLabelText(
      "settings.advanced.plugins.priority: Cache Injector",
    );
    expect(priorityInput).toHaveValue(200);

    await act(async () => {
      fireEvent.change(priorityInput, { target: { value: "120" } });
      fireEvent.blur(priorityInput);
    });

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("plugin_set_priority", {
        id: "builtin:cache-injector",
        priority: 120,
      });
    });
  });

  it("toggles the global master switch via plugin_set_all_enabled", async () => {
    // 全部启用 -> 总开关初始为开；点击后应调用 set_all_enabled(false)
    mockList([builtinPlugin, { ...userPlugin, enabled: true }]);
    render(<PluginSettingsPanel />);

    await waitFor(() => {
      expect(screen.getByText("My Plugin")).toBeInTheDocument();
    });

    const masterSwitch = screen
      .getAllByRole("switch")
      .find(
        (el) => (el as HTMLButtonElement).getAttribute("aria-checked") === "true",
      );
    expect(masterSwitch).toBeDefined();

    await act(async () => {
      fireEvent.click(masterSwitch!);
    });

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("plugin_set_all_enabled", {
        enabled: false,
      });
    });
  });

  it("takes the master switch state from the backend instead of deriving it", async () => {
    // Codex 审查 P1：全局开关必须来自后端显式返回——单个插件被覆盖禁用时
    // 总开关仍为开、行内开关保持可用，不得把用户覆盖误当总开关
    mockList([builtinPlugin, { ...userPlugin, enabled: false }], true);
    render(<PluginSettingsPanel />);

    await waitFor(() => {
      expect(screen.getByText("My Plugin")).toBeInTheDocument();
    });

    const masterSwitch = screen
      .getAllByRole("switch")
      .find(
        (el) => (el as HTMLButtonElement).getAttribute("aria-checked") === "true",
      );
    expect(masterSwitch).toBeDefined();

    const rowSwitches = screen.getAllByRole("switch").slice(1);
    expect(
      rowSwitches.every((el) => !(el as HTMLButtonElement).disabled),
    ).toBe(true);
  });

  it("disables row controls when the backend reports the master switch off", async () => {
    mockList([{ ...builtinPlugin, enabled: false }], false);
    render(<PluginSettingsPanel />);

    await waitFor(() => {
      expect(screen.getByText("Cache Injector")).toBeInTheDocument();
    });

    const rowSwitches = screen.getAllByRole("switch").slice(1);
    expect(
      rowSwitches.every((el) => (el as HTMLButtonElement).disabled),
    ).toBe(true);
  });

  it("reloads plugins and shows returned errors", async () => {
    mockList([builtinPlugin]);
    render(<PluginSettingsPanel />);

    const reloadButton = await screen.findByText("settings.advanced.plugins.reload");
    await act(async () => {
      fireEvent.click(reloadButton);
    });

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("plugin_reload");
    });
    await waitFor(() => {
      expect(
        screen.getByText("settings.advanced.plugins.reloadErrorsTitle"),
      ).toBeInTheDocument();
    });
    expect(screen.getByText("broken: bad manifest")).toBeInTheDocument();
    // 重载后重新拉取列表
    expect(invokeMock).toHaveBeenCalledWith("plugin_list");
  });
});
