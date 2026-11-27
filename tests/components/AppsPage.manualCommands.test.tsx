import { act, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { DndContextProps, DragEndEvent } from "@dnd-kit/core";
import { waitForOverlayTransition } from "../utils/overlayTransition";
import {
  createToolState,
  deferred,
  mountApps,
  registerScrollIntoViewStub,
  renderApps,
  resetAppsPageMocks,
  type ToolVersions,
} from "./appsPageTestHelpers";

const mocks = vi.hoisted(() => ({
  getToolVersions: vi.fn(),
  getConfigStatus: vi.fn(async () => ({
    exists: true,
    path: "/home/me/.workbuddy",
  })),
  probeToolInstallations: vi.fn(),
  listToolInstallations: vi.fn(),
  runToolLifecycleAction: vi.fn(),
  prepareToolUninstall: vi.fn(),
  copyText: vi.fn(),
  info: vi.fn(),
  success: vi.fn(),
  warning: vi.fn(),
  error: vi.fn(),
  settings: {} as { visibleApps?: undefined; appOrder?: string[] },
  updateSettings: vi.fn(),
  autoSaveSettings: vi.fn(async () => null),
  dragEndHandlers: [] as ((event: DragEndEvent) => void)[],
  desktopStatus: { supported: true, installed: true, configured: true },
}));

vi.mock("@/lib/api", () => ({ settingsApi: mocks }));
vi.mock("@/lib/api/providers", () => ({
  providersApi: {
    getClaudeDesktopStatus: async () => mocks.desktopStatus,
  },
}));
vi.mock("@/hooks/useSettings", () => ({
  useSettings: () => ({
    settings: mocks.settings,
    updateSettings: mocks.updateSettings,
    autoSaveSettings: mocks.autoSaveSettings,
  }),
}));
vi.mock("@dnd-kit/core", async () => {
  const actual =
    await vi.importActual<typeof import("@dnd-kit/core")>("@dnd-kit/core");
  return {
    ...actual,
    DndContext: (props: DndContextProps) => {
      if (props.onDragEnd) mocks.dragEndHandlers.push(props.onDragEnd);
      return <actual.DndContext {...props} />;
    },
  };
});
vi.mock("sonner", () => ({ toast: mocks }));
vi.mock("@/lib/clipboard", () => ({ copyText: mocks.copyText }));

const state = createToolState();

registerScrollIntoViewStub();

describe("AppsPage manual install and uninstall commands", () => {
  beforeEach(() => {
    resetAppsPageMocks(mocks, state);
  });

  it("copies only the selected application's manual command and switches installation methods", async () => {
    const getVersions = mocks.getToolVersions.getMockImplementation()!;
    mocks.getToolVersions.mockImplementation(async (tools: string[]) =>
      ((await getVersions(tools)) as ToolVersions).map((tool) => ({
        ...tool,
        env_type: "macos",
      })),
    );
    const user = userEvent.setup();
    await renderApps();
    await user.click(
      screen.getByRole("button", { name: "appsPage.moreActions" }),
    );
    await user.click(
      await screen.findByRole("menuitem", {
        name: "settings.manualUninstallCommands",
      }),
    );
    const dialog = within(screen.getByRole("dialog"));
    const codex = () => within(dialog.getByText("Codex").closest("li")!);
    expect(
      codex().getByText("npm uninstall -g @openai/codex"),
    ).toBeInTheDocument();
    expect(dialog.queryByText("Hermes")).not.toBeInTheDocument();
    await user.click(codex().getByRole("button"));
    expect(mocks.copyText).toHaveBeenLastCalledWith(
      "npm uninstall -g @openai/codex",
    );

    expect(dialog.getByText("macOS")).toBeInTheDocument();
    expect(dialog.getAllByRole("combobox")).toHaveLength(2);
    await user.click(
      dialog.getByRole("combobox", { name: "settings.manualUninstallMethod" }),
    );
    await user.click(
      await screen.findByRole("option", { name: "Homebrew (cask)" }),
    );
    expect(
      dialog.queryByText("npm uninstall -g @openai/codex"),
    ).not.toBeInTheDocument();
    expect(
      dialog.getByText("brew uninstall --cask claude-code"),
    ).toBeInTheDocument();
    expect(
      dialog.getByText("brew uninstall --cask claude-code@latest"),
    ).toBeInTheDocument();
    await user.click(codex().getByRole("button"));
    expect(mocks.copyText).toHaveBeenLastCalledWith(
      "brew uninstall --cask codex",
    );
    expect(mocks.success).toHaveBeenCalledWith(
      "settings.uninstallCommandCopied",
      { closeButton: true },
    );
    expect(mocks.runToolLifecycleAction).not.toHaveBeenCalled();
    await user.click(dialog.getByRole("button", { name: "common.close" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();

    await waitForOverlayTransition();
    await user.click(
      screen.getByRole("button", { name: "appsPage.moreActions" }),
    );
    await user.click(
      await screen.findByRole("menuitem", { name: "appsPage.manualCommands" }),
    );
    expect(
      within(screen.getByRole("dialog")).getByRole("heading", {
        name: "settings.manualInstallCommands",
      }),
    ).toBeInTheDocument();
  });

  it.each([["Windows NT 10.0"], ["X11; Linux x86_64"]])(
    "shows the official Hermes uninstall command for %s",
    async (userAgent) => {
      const platform = vi
        .spyOn(navigator, "userAgent", "get")
        .mockReturnValue(userAgent);
      try {
        const user = userEvent.setup();
        await renderApps();
        await user.click(
          screen.getByRole("button", { name: "appsPage.moreActions" }),
        );
        await user.click(
          await screen.findByRole("menuitem", {
            name: "settings.manualUninstallCommands",
          }),
        );
        const dialog = within(screen.getByRole("dialog"));
        await user.click(
          dialog.getByRole("combobox", {
            name: "settings.manualUninstallMethod",
          }),
        );
        await user.click(await screen.findByRole("option", { name: "Hermes" }));
        expect(
          within(dialog.getByRole("list")).getByText("Hermes"),
        ).toBeInTheDocument();
        expect(dialog.queryByText("Codex")).not.toBeInTheDocument();
        await user.click(
          dialog.getByRole("button", {
            name: "settings.copyToolUninstallCommand",
          }),
        );
        expect(mocks.copyText).toHaveBeenCalledWith("hermes uninstall --yes");
        expect(mocks.runToolLifecycleAction).not.toHaveBeenCalled();
      } finally {
        platform.mockRestore();
      }
    },
  );

  it("keeps manual commands visible when copying fails", async () => {
    mocks.copyText.mockRejectedValueOnce(new Error("clipboard unavailable"));
    const user = userEvent.setup();
    await renderApps();
    await user.click(
      screen.getByRole("button", { name: "appsPage.moreActions" }),
    );
    await user.click(
      await screen.findByRole("menuitem", {
        name: "settings.manualUninstallCommands",
      }),
    );
    const dialog = within(screen.getByRole("dialog"));
    const codex = within(dialog.getByText("Codex").closest("li")!);
    await user.click(codex.getByRole("button"));
    expect(mocks.error).toHaveBeenCalledWith(
      "settings.uninstallCommandCopyFailed",
    );
    expect(
      codex.getByText("npm uninstall -g @openai/codex"),
    ).toBeInTheDocument();
    expect(mocks.success).not.toHaveBeenCalled();
    expect(mocks.runToolLifecycleAction).not.toHaveBeenCalled();
  });

  it("automatically detects Windows and only shows documented pnpm and Bun clients", async () => {
    const platform = vi
      .spyOn(navigator, "userAgent", "get")
      .mockReturnValue("Windows NT 10.0");
    try {
      const user = userEvent.setup();
      await renderApps();
      await user.click(
        screen.getByRole("button", { name: "appsPage.moreActions" }),
      );
      await user.click(
        await screen.findByRole("menuitem", {
          name: "settings.manualUninstallCommands",
        }),
      );
      const dialog = within(screen.getByRole("dialog"));
      expect(dialog.getByText("Windows")).toBeInTheDocument();
      expect(dialog.getAllByRole("combobox")).toHaveLength(2);
      const chooseMethod = async (name: string) => {
        await user.click(
          dialog.getByRole("combobox", {
            name: "settings.manualUninstallMethod",
          }),
        );
        expect(
          screen.queryByRole("option", { name: "Volta" }),
        ).not.toBeInTheDocument();
        await user.click(await screen.findByRole("option", { name }));
      };
      await user.click(
        dialog.getByRole("combobox", {
          name: "settings.manualUninstallMethod",
        }),
      );
      expect(
        screen.queryByRole("option", { name: "Homebrew (cask)" }),
      ).not.toBeInTheDocument();
      expect(
        screen.queryByRole("option", { name: "Homebrew (formula)" }),
      ).not.toBeInTheDocument();
      await user.click(await screen.findByRole("option", { name: "pnpm" }));
      expect(dialog.getAllByRole("listitem")).toHaveLength(3);
      expect(
        dialog.getByText("pnpm remove -g opencode-ai"),
      ).toBeInTheDocument();
      // OpenCode 2 is a separate package.
      expect(
        dialog.getByText("pnpm remove -g @opencode/cli"),
      ).toBeInTheDocument();
      expect(dialog.getByText("pnpm remove -g openclaw")).toBeInTheDocument();
      expect(dialog.queryByText("Codex")).not.toBeInTheDocument();
      await chooseMethod("bun");
      expect(dialog.getAllByRole("listitem")).toHaveLength(2);
      expect(dialog.getByText("bun remove -g openclaw")).toBeInTheDocument();
      expect(
        dialog.getByText("bun remove -g @oh-my-pi/pi-coding-agent"),
      ).toBeInTheDocument();
      expect(
        dialog.queryByText("bun remove -g opencode-ai"),
      ).not.toBeInTheDocument();
      expect(
        dialog.queryByText("bun remove -g @opencode/cli"),
      ).not.toBeInTheDocument();
      expect(
        dialog.getByText("settings.manualUninstallOpenClawHint"),
      ).toBeInTheDocument();
      expect(
        dialog.getByText("settings.manualUninstallBunLauncherHint"),
      ).toBeInTheDocument();

      expect(mocks.runToolLifecycleAction).not.toHaveBeenCalled();
    } finally {
      platform.mockRestore();
    }
  });

  it.each([
    ["updates the detected environments and per-client WSL commands", false],
    ["preserves a manual environment selection during detection", true],
  ] as const)("%s", async (_name, manualOverride) => {
    const platform = vi
      .spyOn(navigator, "userAgent", "get")
      .mockReturnValue("Windows NT 10.0");
    try {
      const pending = deferred<ToolVersions>();
      const getVersions = mocks.getToolVersions.getMockImplementation()!;
      const opencode = (await getVersions(["opencode"])) as ToolVersions;
      mocks.getToolVersions.mockImplementation((tools: string[]) =>
        tools.includes("opencode") ? pending.promise : getVersions(tools),
      );
      const user = userEvent.setup();
      const { AppsPage } = await import("@/components/apps/AppsPage");
      mountApps(AppsPage);
      await user.click(
        screen.getByRole("button", { name: "appsPage.moreActions" }),
      );
      await user.click(
        await screen.findByRole("menuitem", {
          name: "settings.manualUninstallCommands",
        }),
      );
      const dialog = within(screen.getByRole("dialog"));
      const environmentSelect = dialog.getByRole("combobox", {
        name: "settings.manualUninstallEnvironment",
      });
      expect(environmentSelect).toHaveTextContent("Windows");
      await user.click(
        dialog.getByRole("combobox", {
          name: "settings.manualUninstallMethod",
        }),
      );
      await user.click(await screen.findByRole("option", { name: "bun" }));
      expect(
        dialog.queryByText("bun remove -g opencode-ai"),
      ).not.toBeInTheDocument();

      const chooseEnvironment = async (name: string) => {
        await user.click(environmentSelect);
        expect(
          (await screen.findAllByRole("option")).map(
            (option) => option.textContent,
          ),
        ).toEqual(["Windows", "macOS", "Linux / WSL"]);
        await user.click(await screen.findByRole("option", { name }));
      };
      // Selecting the currently detected Windows environment explicitly makes
      // it an override; the pending WSL detection must not replace that choice.
      if (manualOverride) await chooseEnvironment("Windows");
      await act(async () =>
        pending.resolve(
          opencode.map((tool) => ({
            ...tool,
            env_type: "wsl",
            wsl_distro: "Ubuntu",
          })),
        ),
      );
      if (manualOverride) {
        expect(environmentSelect).toHaveTextContent(/^Windows$/);
        expect(
          dialog.queryByText("bun remove -g opencode-ai"),
        ).not.toBeInTheDocument();
      } else {
        expect(environmentSelect).toHaveTextContent("Windows / WSL · Ubuntu");
        const opencodeRow = within(
          dialog.getByText("bun remove -g opencode-ai").closest("li")!,
        );
        expect(opencodeRow.getByText("WSL · Ubuntu")).toBeInTheDocument();
        const openclawRow = within(
          dialog.getByText("bun remove -g openclaw").closest("li")!,
        );
        expect(openclawRow.getByText("Windows")).toBeInTheDocument();
        expect(dialog.getAllByRole("combobox")).toHaveLength(2);

        await user.click(
          dialog.getByRole("combobox", {
            name: "settings.manualUninstallMethod",
          }),
        );
        expect(
          screen.queryByRole("option", { name: "Homebrew (cask)" }),
        ).not.toBeInTheDocument();
        await user.click(
          await screen.findByRole("option", { name: "Homebrew (formula)" }),
        );
        expect(
          dialog.getByText("brew uninstall --formula opencode"),
        ).toBeInTheDocument();
        expect(
          dialog.getByText("brew uninstall --formula anomalyco/tap/opencode"),
        ).toBeInTheDocument();
        expect(
          dialog.queryByText("brew uninstall --formula gemini-cli"),
        ).not.toBeInTheDocument();
      }

      await chooseEnvironment("macOS");
      await user.click(
        dialog.getByRole("combobox", {
          name: "settings.manualUninstallMethod",
        }),
      );
      await user.click(
        await screen.findByRole("option", { name: "Homebrew (cask)" }),
      );
      expect(
        dialog.getByText("brew uninstall --cask codex"),
      ).toBeInTheDocument();
      expect(dialog.queryByText("WSL · Ubuntu")).not.toBeInTheDocument();

      // A manual switch also drops methods unsupported in the new environment.
      await chooseEnvironment("Windows");
      expect(
        dialog.getByRole("combobox", {
          name: "settings.manualUninstallMethod",
        }),
      ).toHaveTextContent("npm");
      expect(
        dialog.queryByText("brew uninstall --cask codex"),
      ).not.toBeInTheDocument();
      await chooseEnvironment("Linux / WSL");
      await user.click(
        dialog.getByRole("combobox", {
          name: "settings.manualUninstallMethod",
        }),
      );
      await user.click(await screen.findByRole("option", { name: "bun" }));
      expect(dialog.getByText("bun remove -g opencode-ai")).toBeInTheDocument();
      expect(mocks.runToolLifecycleAction).not.toHaveBeenCalled();
    } finally {
      platform.mockRestore();
    }
  });

  it("only offers valid Homebrew formula uninstall commands", async () => {
    const getVersions = mocks.getToolVersions.getMockImplementation()!;
    mocks.getToolVersions.mockImplementation(async (tools: string[]) =>
      ((await getVersions(tools)) as ToolVersions).map((tool) => ({
        ...tool,
        env_type: "linux",
      })),
    );
    const user = userEvent.setup();
    await renderApps();
    await user.click(
      screen.getByRole("button", { name: "appsPage.moreActions" }),
    );
    await user.click(
      await screen.findByRole("menuitem", {
        name: "settings.manualUninstallCommands",
      }),
    );
    const dialog = within(screen.getByRole("dialog"));
    expect(dialog.getByText("Linux")).toBeInTheDocument();
    await user.click(
      dialog.getByRole("combobox", { name: "settings.manualUninstallMethod" }),
    );
    await user.click(
      await screen.findByRole("option", { name: "Homebrew (formula)" }),
    );
    expect(
      dialog.getByText("brew uninstall --formula gemini-cli"),
    ).toBeInTheDocument();
    expect(
      dialog.getByText("brew uninstall --formula opencode"),
    ).toBeInTheDocument();
    expect(
      dialog.getByText("brew uninstall --formula anomalyco/tap/opencode"),
    ).toBeInTheDocument();
    expect(dialog.queryByText("Codex")).toBeNull();
    expect(dialog.queryByText("Claude Code")).toBeNull();
    expect(dialog.queryByText("Grok Build")).toBeNull();
  });
});
