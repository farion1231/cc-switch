import { vi, expect, it, describe } from "vitest";
import { act, screen, within, waitFor } from "@testing-library/react";
import { waitForOverlayTransition } from "../utils/overlayTransition";
import {
  mocks,
  deferred,
  report,
  candidate,
  missing,
  card,
  mountApps,
  renderApps,
} from "./appsPageTestHelpers";
import type { ToolVersions } from "./appsPageTestHelpers";
import userEvent from "@testing-library/user-event";

describe("AppsPage install commands", () => {
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
        name: "appsPage.manualCommands",
      }),
    ).toBeInTheDocument();
    const installDialog = within(screen.getByRole("dialog"));
    await user.click(
      installDialog.getByRole("button", {
        name: "appsPage.installOfficialSite",
      }),
    );
    expect(mocks.openExternal).toHaveBeenLastCalledWith(
      "https://claude.com/product/claude-code",
    );
    await user.click(
      installDialog.getByRole("button", { name: "Antigravity CLI" }),
    );
    await user.click(
      installDialog.getByRole("button", {
        name: "appsPage.installOfficialSite",
      }),
    );
    expect(mocks.openExternal).toHaveBeenLastCalledWith(
      "https://antigravity.google/download#antigravity-cli",
    );
    expect(mocks.runToolLifecycleAction).not.toHaveBeenCalled();
  });

  it("links to both official WorkBuddy downloads without running a CLI install", async () => {
    const user = userEvent.setup();
    mocks.getConfigStatus.mockResolvedValueOnce({
      exists: false,
      path: "/home/me/.workbuddy",
    });
    await renderApps();
    await user.click(
      card("WorkBuddy").getByRole("button", { name: "settings.toolInstall" }),
    );
    const dialog = within(screen.getByRole("dialog"));
    expect(dialog.getByRole("heading", { name: "WorkBuddy" })).toBeVisible();
    expect(dialog.getByText("appsPage.installDownloadRequired")).toBeVisible();
    expect(dialog.queryByRole("button", { name: /^common.copy/ })).toBeNull();
    await user.click(
      dialog.getByRole("button", { name: "appsPage.installDownloadChina" }),
    );
    expect(mocks.openExternal).toHaveBeenLastCalledWith(
      "https://www.workbuddy.cn/",
    );
    await user.click(
      dialog.getByRole("button", { name: "appsPage.installDownloadGlobal" }),
    );
    expect(mocks.openExternal).toHaveBeenLastCalledWith(
      "https://www.workbuddy.ai/",
    );
    await user.click(dialog.getByRole("button", { name: "MiniMax Code" }));
    await user.click(
      dialog.getByRole("button", { name: "appsPage.installDownloadChina" }),
    );
    expect(mocks.openExternal).toHaveBeenLastCalledWith(
      "https://code.minimax.cn/about",
    );
    await user.click(
      dialog.getByRole("button", { name: "appsPage.installDownloadGlobal" }),
    );
    expect(mocks.openExternal).toHaveBeenLastCalledWith(
      "https://code.minimax.io/about",
    );
    expect(mocks.runToolLifecycleAction).not.toHaveBeenCalled();
  });

  it("keeps stable commands out of the WSL OpenCode prerelease update guide", async () => {
    const platform = vi
      .spyOn(navigator, "userAgent", "get")
      .mockReturnValue("Windows NT 10.0");
    try {
      const original = mocks.getToolVersions.getMockImplementation()!;
      mocks.getToolVersions.mockImplementation(async (tools: string[]) =>
        (await original(tools)).map((result: ToolVersions[number]) =>
          result.name === "opencode"
            ? {
                ...result,
                env_type: "wsl",
                wsl_distro: "Ubuntu",
                version: "2.3.2-next.1",
                latest_version: null,
                manual_update_only: true,
              }
            : result,
        ),
      );
      await renderApps();
      await userEvent.click(
        card("OpenCode").getByRole("button", {
          name: "appsPage.manualUpdateGuide",
        }),
      );
      const dialog = within(screen.getByRole("dialog"));
      expect(
        dialog.getByText("appsPage.installPrereleaseUpdate"),
      ).toBeVisible();
      expect(
        dialog.queryAllByRole("button", { name: /^common.copy / }),
      ).toHaveLength(0);
      expect(
        dialog.queryAllByRole("button", { name: /^settings.toolInstall / }),
      ).toHaveLength(0);
      expect(
        dialog.getByRole("button", { name: "appsPage.installOfficialSite" }),
      ).toBeVisible();
      expect(mocks.runToolLifecycleAction).not.toHaveBeenCalled();
    } finally {
      platform.mockRestore();
    }
  });

  it("shows an update guide when a detected Windows OpenCode update is available", async () => {
    const platform = vi
      .spyOn(navigator, "userAgent", "get")
      .mockReturnValue("Windows NT 10.0");
    try {
      const getVersions = mocks.getToolVersions.getMockImplementation()!;
      mocks.getToolVersions.mockImplementation(async (tools: string[]) =>
        (await getVersions(tools)).map((result: ToolVersions[number]) =>
          result.name === "opencode"
            ? {
                ...result,
                env_type: "windows",
                version: "2.3.1",
                latest_version: "2.3.2",
                manual_update_only: true,
              }
            : result,
        ),
      );
      await renderApps();
      const row = card("OpenCode");
      expect(row.getByText("appsPage.newVersion")).toBeVisible();
      expect(
        row.queryByRole("button", { name: "settings.toolUpdate" }),
      ).toBeNull();
      await userEvent.click(
        row.getByRole("button", { name: "appsPage.manualUpdateGuide" }),
      );
      const dialog = within(screen.getByRole("dialog"));
      expect(dialog.getByRole("heading", { name: "OpenCode" })).toBeVisible();
      expect(dialog.getByText("appsPage.installManualUpdate")).toBeVisible();
      expect(
        dialog.getAllByRole("button", { name: /^common.copy / }).length,
      ).toBeGreaterThan(0);
      expect(
        dialog.queryByRole("button", { name: /^settings.toolInstall / }),
      ).toBeNull();
      await userEvent.click(dialog.getByRole("button", { name: "WSL" }));
      expect(
        dialog.getByRole("button", { name: "common.copy npm ≤ 11.15" }),
      ).toBeEnabled();
      expect(mocks.runToolLifecycleAction).not.toHaveBeenCalled();
    } finally {
      platform.mockRestore();
    }
  });

  it("keeps the OpenCode catalog visible while directing manual WSL updates to the original installer", async () => {
    const platform = vi
      .spyOn(navigator, "userAgent", "get")
      .mockReturnValue("Windows NT 10.0");
    try {
      const getVersions = mocks.getToolVersions.getMockImplementation()!;
      mocks.getToolVersions.mockImplementation(async (tools: string[]) =>
        (await getVersions(tools)).map((result: ToolVersions[number]) =>
          result.name === "opencode"
            ? {
                ...result,
                version: "1.19.6",
                latest_version: "1.20.0",
                env_type: "wsl",
                wsl_distro: "Ubuntu",
                manual_update_only: true,
              }
            : result,
        ),
      );
      await renderApps();
      const row = card("OpenCode");
      expect(
        row.queryByRole("button", { name: "settings.toolUpdate" }),
      ).toBeNull();
      await userEvent.click(
        row.getByRole("button", { name: "appsPage.manualUpdateGuide" }),
      );
      const dialog = within(screen.getByRole("dialog"));
      expect(dialog.getByText("WSL · Ubuntu")).toBeVisible();
      expect(
        dialog.getByRole("button", { name: "appsPage.installOfficialSite" }),
      ).toBeEnabled();
      expect(
        dialog.getAllByRole("button", { name: /^common.copy / }).length,
      ).toBeGreaterThan(0);
      expect(
        dialog.queryByRole("button", { name: /^settings.toolInstall / }),
      ).toBeNull();
      expect(dialog.getByText("appsPage.installManualUpdate")).toBeVisible();
      await userEvent.click(dialog.getByRole("button", { name: "Windows" }));
      expect(
        dialog.getByRole("button", { name: "common.copy npm (OpenCode 1)" }),
      ).toBeEnabled();
      expect(mocks.runToolLifecycleAction).not.toHaveBeenCalled();
    } finally {
      platform.mockRestore();
    }
  });

  it("keeps DSH commands in the catalog and shows the detected npm channel", async () => {
    const platform = vi
      .spyOn(navigator, "userAgent", "get")
      .mockReturnValue("Macintosh; Intel Mac OS X");
    try {
      const getVersions = mocks.getToolVersions.getMockImplementation()!;
      mocks.getToolVersions.mockImplementation(async (tools: string[]) =>
        (await getVersions(tools)).map((result: ToolVersions[number]) =>
          result.name === "dsh"
            ? {
                ...result,
                env_type: "macos",
                version: "0.2.2-rc.5",
                latest_version: "0.2.2-rc.5",
                manual_update_only: true,
              }
            : result,
        ),
      );
      mocks.getToolInstallationReports.mockImplementation(
        async (tools: string[]) =>
          tools.map((tool) =>
            report(tool, {
              installs:
                tool === "dsh"
                  ? [
                      candidate(null, {
                        source: "npm",
                        path: "/qa/bin/dsh",
                        version: "0.2.2-rc.5",
                      }),
                      candidate(null, {
                        source: "npm",
                        path: "/qa/alternate/bin/dsh",
                        version: "0.2.1",
                      }),
                    ]
                  : [],
            }),
          ),
      );
      await renderApps();
      const user = userEvent.setup();
      const row = card("DeepSeek Harness");
      const versions = within(
        await row.findByRole("list", { name: "appsPage.columnVersion" }),
      );
      expect(versions.getByText("0.2.2-rc.5")).toBeVisible();
      expect(versions.getByText("0.2.1")).toBeVisible();
      expect(
        row.queryByRole("button", { name: "appsPage.manualCommands" }),
      ).toBeNull();
      expect(
        row.queryByRole("button", { name: "settings.toolInstall" }),
      ).toBeNull();
      expect(
        row.queryByRole("button", { name: "appsPage.manualUpdateGuide" }),
      ).toBeNull();
      await user.click(
        screen.getByRole("button", { name: "appsPage.moreActions" }),
      );
      await user.click(
        await screen.findByRole("menuitem", {
          name: "appsPage.manualCommands",
        }),
      );
      const dialog = within(screen.getByRole("dialog"));
      await user.click(
        dialog.getByRole("button", { name: "DeepSeek Harness" }),
      );
      const installed = (
        await dialog.findAllByText("appsPage.installAlreadyInstalled")
      )[0];
      expect(within(installed.parentElement!).getByText("npm")).toBeVisible();
      expect(
        dialog.getByRole("button", { name: "common.copy npm" }),
      ).toBeEnabled();
      await user.click(
        dialog.getByRole("button", { name: "common.copy npx (Web UI)" }),
      );
      expect(mocks.copyText).toHaveBeenCalledWith("npx @deepseek-ai/dsh web");
      expect(mocks.runToolLifecycleAction).not.toHaveBeenCalled();
    } finally {
      platform.mockRestore();
    }
  });

  it.each([
    {
      envType: "macos",
      platformLabel: "macOS",
      userAgent: "Mozilla/5.0 (Macintosh; Intel Mac OS X)",
      navigatorPlatform: "MacIntel",
    },
    {
      envType: "linux",
      platformLabel: "Linux",
      userAgent: "Mozilla/5.0 (X11; Linux x86_64)",
      navigatorPlatform: "Linux x86_64",
    },
    {
      envType: "windows",
      platformLabel: "Windows",
      userAgent: "Mozilla/5.0 (Windows NT 10.0; Win64; x64)",
      navigatorPlatform: "Win32",
    },
  ])(
    "keeps npm installation available for another global prefix on $envType",
    async ({ envType, platformLabel, userAgent, navigatorPlatform }) => {
      const agent = vi
        .spyOn(navigator, "userAgent", "get")
        .mockReturnValue(userAgent);
      const platform = vi
        .spyOn(navigator, "platform", "get")
        .mockReturnValue(navigatorPlatform);
      try {
        const getVersions = mocks.getToolVersions.getMockImplementation()!;
        mocks.getToolVersions.mockImplementation(async (tools: string[]) =>
          (await getVersions(tools)).map((result: ToolVersions[number]) => ({
            ...result,
            env_type: envType,
          })),
        );
        mocks.getToolInstallationReports.mockImplementation(
          async (tools: string[]) =>
            tools.map((tool) =>
              report(tool, {
                installs:
                  tool === "codebuddy"
                    ? [
                        candidate(null, {
                          source: "npm",
                          path:
                            envType === "windows"
                              ? "C:/qa/old-prefix/codebuddy.cmd"
                              : "/qa/old-prefix/bin/codebuddy",
                          version: "1.0.0",
                        }),
                      ]
                    : [],
              }),
            ),
        );
        await renderApps();
        const user = userEvent.setup();
        await user.click(
          screen.getByRole("button", { name: "appsPage.moreActions" }),
        );
        await user.click(
          await screen.findByRole("menuitem", {
            name: "appsPage.manualCommands",
          }),
        );
        const dialog = within(screen.getByRole("dialog"));
        await user.click(dialog.getByRole("button", { name: "CodeBuddy" }));
        expect(
          dialog.getByRole("button", { name: "settings.toolInstall npm" }),
        ).toBeEnabled();
        expect(
          dialog.getByText(platformLabel, { selector: "p" }),
        ).toBeVisible();
        await waitFor(() =>
          expect(
            dialog.getAllByText("appsPage.installAlreadyInstalled").length,
          ).toBeGreaterThan(1),
        );
      } finally {
        agent.mockRestore();
        platform.mockRestore();
      }
    },
  );

  it("disables npm installation when its runtime is missing and retains copy", async () => {
    mocks.getInstallManagerAvailability.mockResolvedValue({
      npm: true,
      node: false,
    });
    await renderApps();
    const user = userEvent.setup();
    await user.click(
      screen.getByRole("button", { name: "appsPage.moreActions" }),
    );
    await user.click(
      await screen.findByRole("menuitem", { name: "appsPage.manualCommands" }),
    );
    const dialog = within(screen.getByRole("dialog"));
    await user.click(dialog.getByRole("button", { name: "CodeBuddy" }));
    await waitFor(() =>
      expect(
        dialog.getByRole("button", { name: "settings.toolInstall npm" }),
      ).toBeDisabled(),
    );
    expect(
      dialog.getByRole("button", { name: "common.copy npm" }),
    ).toBeEnabled();
    expect(mocks.runToolLifecycleAction).not.toHaveBeenCalled();
  });

  it("offers the original package manager for a manual-only Linux OpenCode update", async () => {
    const platform = vi
      .spyOn(navigator, "userAgent", "get")
      .mockReturnValue("X11; Linux x86_64");
    const platformName = vi
      .spyOn(navigator, "platform", "get")
      .mockReturnValue("Linux x86_64");
    try {
      const getVersions = mocks.getToolVersions.getMockImplementation()!;
      mocks.getToolVersions.mockImplementation(async (tools: string[]) =>
        (await getVersions(tools)).map((result: ToolVersions[number]) =>
          result.name === "opencode"
            ? {
                ...result,
                env_type: "linux",
                version: "2.3.1",
                latest_version: null,
                manual_update_only: true,
              }
            : result,
        ),
      );
      await renderApps();
      const row = card("OpenCode");
      expect(
        row.queryByRole("button", { name: "settings.toolUpdate" }),
      ).toBeNull();
      await userEvent.click(
        row.getByRole("button", { name: "appsPage.manualUpdateGuide" }),
      );
      const dialog = within(screen.getByRole("dialog"));
      expect(dialog.getByText("appsPage.installManualUpdate")).toBeVisible();
      expect(
        dialog.getAllByRole("button", { name: /^common.copy / }).length,
      ).toBeGreaterThan(0);
      expect(
        dialog.queryByRole("button", { name: /^settings.toolInstall / }),
      ).toBeNull();
      expect(
        dialog.queryByRole("button", { name: /^settings.toolInstall / }),
      ).toBeNull();
    } finally {
      platform.mockRestore();
      platformName.mockRestore();
    }
  });

  it("waits for WSL detection before offering copyable install commands", async () => {
    const platform = vi
      .spyOn(navigator, "userAgent", "get")
      .mockReturnValue("Windows NT 10.0");
    const detection = deferred<ToolVersions>();
    mocks.getToolVersions.mockImplementation(() => detection.promise);
    try {
      const user = userEvent.setup();
      const { AppsPage } = await import("@/components/apps/AppsPage");
      mountApps(AppsPage);
      await user.click(
        screen.getByRole("button", { name: "appsPage.moreActions" }),
      );
      await user.click(
        await screen.findByRole("menuitem", {
          name: "appsPage.manualCommands",
        }),
      );
      const dialog = within(screen.getByRole("dialog"));
      expect(dialog.getByRole("status")).toHaveTextContent(
        "appsPage.installEnvironmentPending",
      );
      expect(dialog.queryByRole("button", { name: /^common.copy/ })).toBeNull();
      await user.click(dialog.getByRole("button", { name: "Windows" }));
      expect(
        dialog.getByRole("button", { name: "common.copy PowerShell" }),
      ).toBeEnabled();
      await user.click(dialog.getByRole("button", { name: "WSL" }));
      expect(
        dialog.getByRole("button", { name: "common.copy curl" }),
      ).toBeEnabled();
      expect(
        dialog.getByText("appsPage.installWslDetectionHint"),
      ).toBeVisible();
      await act(async () =>
        detection.resolve([
          {
            name: "claude",
            version: null,
            latest_version: null,
            error: null,
            installed_but_broken: false,
            env_type: "wsl",
            wsl_distro: "Ubuntu",
            update_blocked_by_node: null,
          },
        ]),
      );
      expect(
        dialog.queryByRole("button", { name: /^settings.toolInstall / }),
      ).toBeNull();
      expect(dialog.getByText("WSL · Ubuntu")).toBeVisible();
      expect(
        dialog.getByRole("button", { name: "common.copy curl" }),
      ).toBeEnabled();
      expect(
        dialog.queryByRole("button", { name: "common.copy PowerShell" }),
      ).toBeNull();
    } finally {
      platform.mockRestore();
    }
  });

  it.each([
    ["Macintosh", false, false],
    ["X11; Linux x86_64", true, false],
    ["Windows NT 10.0", false, true],
    ["Windows NT 10.0 (WSL)", true, false],
  ])(
    "filters OpenCode install channels for %s",
    async (userAgent, hasAur, downloadsOnly) => {
      const platform = vi
        .spyOn(navigator, "userAgent", "get")
        .mockReturnValue(userAgent);
      try {
        missing.add("claude");
        missing.add("opencode");
        missing.add("copilot");
        const wsl = userAgent.endsWith("(WSL)");
        if (wsl) {
          const getVersions = mocks.getToolVersions.getMockImplementation()!;
          mocks.getToolVersions.mockImplementation(async (tools: string[]) =>
            (await getVersions(tools)).map((result: ToolVersions[number]) =>
              result.name === "opencode"
                ? { ...result, env_type: "wsl", wsl_distro: "Ubuntu" }
                : result,
            ),
          );
        }
        const user = userEvent.setup();
        await renderApps();
        await user.click(
          screen.getByRole("button", { name: "appsPage.moreActions" }),
        );
        await user.click(
          await screen.findByRole("menuitem", {
            name: "appsPage.manualCommands",
          }),
        );
        const dialog = within(screen.getByRole("dialog"));
        if (userAgent.startsWith("Windows")) {
          expect(
            dialog.getByText(
              "winget install --id Anthropic.ClaudeCode --exact",
            ),
          ).toBeVisible();
          expect(
            dialog.queryByText("brew install --cask claude-code"),
          ).toBeNull();
        }
        const search = dialog.getByRole("searchbox", {
          name: "appsPage.installCommandApp",
        });
        const appList = within(
          dialog.getByRole("list", { name: "appsPage.manualCommands" }),
        );
        const names = appList
          .getAllByRole("button")
          .map((button) => button.lastElementChild?.textContent ?? "");
        expect(names).toEqual(
          [...names].sort((a, b) => a.localeCompare(b, "en")),
        );
        await user.type(search, "NO-SUCH-APP");
        expect(appList.getByRole("status")).toHaveTextContent(
          "appsPage.installAppNotFound",
        );
        await user.clear(search);
        await user.type(search, "OPENCODE");
        expect(appList.getAllByRole("button")).toHaveLength(1);
        await user.click(appList.getByRole("button", { name: "OpenCode" }));
        if (wsl) expect(dialog.getByText("WSL · Ubuntu")).toBeVisible();
        expect(dialog.queryByText("paru -S opencode-beta") !== null).toBe(
          hasAur,
        );
        await user.click(
          dialog.getByRole("button", { name: "appsPage.installOfficialSite" }),
        );
        expect(mocks.openExternal).toHaveBeenLastCalledWith(
          "https://opencode.ai/docs/#cli",
        );
        expect(dialog.queryByText(/claude-code@latest/)).toBeNull();
        if (!downloadsOnly) {
          await user.click(
            dialog.getByRole("button", { name: "common.copy npm ≤ 11.15" }),
          );
          expect(mocks.copyText).toHaveBeenLastCalledWith(
            "npm install -g --ignore-scripts=false @opencode/cli",
          );
        } else {
          expect(
            dialog.queryByText(
              "npm install -g --ignore-scripts=false @opencode/cli",
            ),
          ).toBeNull();
          await user.click(
            dialog.getByRole("button", {
              name: "common.copy npm (OpenCode 1)",
            }),
          );
          expect(mocks.copyText).toHaveBeenLastCalledWith(
            "npm install -g opencode-ai@latest --ignore-scripts=false --include=optional --allow-scripts=opencode-ai",
          );
        }
        await user.clear(search);
        await user.type(search, "GitHub Copilot CLI");
        await user.click(
          appList.getByRole("button", { name: "GitHub Copilot CLI" }),
        );
        expect(
          dialog.queryByText("brew install --cask copilot-cli") !== null,
        ).toBe(userAgent === "Macintosh" || userAgent.startsWith("X11"));
        expect(
          dialog.queryByRole("button", {
            name: "settings.toolInstall Homebrew",
          }) !== null,
        ).toBe(userAgent === "Macintosh");
        expect(mocks.runToolLifecycleAction).not.toHaveBeenCalled();
      } finally {
        platform.mockRestore();
      }
    },
  );

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
      expect(dialog.getAllByRole("listitem")).toHaveLength(4);
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
      expect(dialog.getAllByRole("listitem")).toHaveLength(3);
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
        await waitForOverlayTransition();
        await waitFor(() => expect(environmentSelect).toHaveTextContent(name));
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
          dialog.getByText("bun remove -g @opencode/cli").closest("li")!,
        );
        expect(opencodeRow.getByText("WSL · Ubuntu")).toBeInTheDocument();
        expect(
          opencodeRow.getByRole("button", {
            name: "settings.copyToolUninstallCommand",
          }),
        ).toBeVisible();
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
});
