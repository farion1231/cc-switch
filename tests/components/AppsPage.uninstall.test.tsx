import { vi, expect, it, describe } from "vitest";
import {
  act,
  fireEvent,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import type { ToolUninstallCandidate } from "@/lib/api/settings";
import { waitForOverlayTransition } from "../utils/overlayTransition";
import {
  mocks,
  deferred,
  report,
  uninstallPlan,
  candidate,
  upgraded,
  missing,
  card,
  rowMenu,
  openUninstall,
  updateButton,
  renderApps,
} from "./appsPageTestHelpers";
import type { ToolVersions } from "./appsPageTestHelpers";
import userEvent from "@testing-library/user-event";

describe("AppsPage uninstall flows", () => {
  it("shows the selected path and command while retaining the resolved target for uninstall validation", async () => {
    const pending = deferred<ToolUninstallCandidate[]>();
    mocks.prepareToolUninstall.mockReturnValueOnce(pending.promise);
    await renderApps();
    await openUninstall("Claude Code");
    const dialog = within(screen.getByRole("dialog"));
    expect(dialog.getByRole("status")).toHaveTextContent(
      "settings.toolUninstallPreparing",
    );
    expect(
      dialog.queryByRole("button", { name: "settings.toolUninstall" }),
    ).toBeNull();
    expect(mocks.runToolLifecycleAction).not.toHaveBeenCalled();
    const plan = {
      ...uninstallPlan("claude"),
      real_path: "C:/Users/me/.local/share/claude/versions/2.0.0",
      native: true,
    };
    await act(async () => pending.resolve([candidate(plan)]));
    expect(dialog.getByText(plan.path)).toBeInTheDocument();
    expect(dialog.queryByText(plan.real_path)).not.toBeInTheDocument();
    expect(dialog.getByText(plan.command)).toBeInTheDocument();
    expect(
      dialog.getByText("settings.toolUninstallNativeHint"),
    ).toBeInTheDocument();
    expect(mocks.prepareToolUninstall).toHaveBeenCalledWith("claude", {});
    fireEvent.click(
      dialog.getByRole("button", { name: "settings.toolUninstall" }),
    );
    await waitFor(() =>
      expect(mocks.runToolLifecycleAction).toHaveBeenCalledWith(
        ["claude"],
        "uninstall",
        {},
        plan,
      ),
    );
  });

  it("offers only manual uninstall when a plan cannot be prepared", async () => {
    mocks.prepareToolUninstall.mockRejectedValueOnce(
      new Error("Unverified Homebrew owner: mise"),
    );
    await renderApps();
    await openUninstall("Codex");
    const dialog = within(screen.getByRole("dialog"));
    expect(await dialog.findByRole("alert")).toHaveTextContent(
      "settings.toolUninstallManualRequired",
    );
    expect(
      dialog.getByText("Unverified Homebrew owner: mise"),
    ).toBeInTheDocument();
    expect(
      dialog.queryByRole("button", { name: "settings.toolUninstall" }),
    ).toBeNull();
    fireEvent.click(
      dialog.getByRole("button", { name: "settings.manualUninstallCommands" }),
    );
    expect(
      screen.getByRole("heading", { name: "settings.manualUninstallCommands" }),
    ).toBeInTheDocument();
    expect(mocks.runToolLifecycleAction).not.toHaveBeenCalled();
  });

  it("lists every installation and uninstalls only the one the user picks", async () => {
    const native = {
      ...uninstallPlan("grok"),
      path: "/Users/me/.grok/bin/grok",
      real_path: "/Users/me/.grok/bin/grok-1.0.50",
      command:
        "rm -- '/Users/me/.grok/bin/grok' '/Users/me/.grok/bin/grok-1.0.50'",
      native: true,
    };
    const npm = {
      ...uninstallPlan("grok"),
      path: "/Users/me/.nvm/versions/node/v22/bin/grok",
      command:
        "npm uninstall -g --prefix /Users/me/.nvm/versions/node/v22 @xai-official/grok",
    };
    mocks.prepareToolUninstall.mockResolvedValueOnce([
      candidate(native, { version: "1.0.50" }),
      candidate(npm, {
        version: "1.0.50",
        source: "nvm",
        is_path_default: false,
        reinstalled_by_updater: true,
      }),
      candidate(null, {
        path: "/opt/tools/grok",
        version: "0.9.0",
        is_path_default: false,
        unavailable_reason:
          "Cannot automatically uninstall Grok Build at /opt/tools/grok.",
      }),
    ]);
    await renderApps();
    await openUninstall("Grok Build");
    const dialog = within(screen.getByRole("dialog"));
    const npmRadio = await dialog.findByRole("radio", { name: npm.path });
    const nativeRadio = dialog.getByRole("radio", { name: native.path });
    const manualRadio = dialog.getByRole("radio", { name: "/opt/tools/grok" });
    expect(nativeRadio).not.toBeChecked();
    expect(npmRadio).not.toBeChecked();
    expect(manualRadio).toBeDisabled();
    expect(
      dialog.getByText(
        "Cannot automatically uninstall Grok Build at /opt/tools/grok.",
      ),
    ).toBeInTheDocument();
    expect(
      dialog.getByText("settings.toolUninstallReinstalledByUpdater"),
    ).toBeInTheDocument();
    expect(
      dialog.getByRole("button", { name: "settings.manualUninstallCommands" }),
    ).toBeInTheDocument();
    const confirm = dialog.getByRole("button", {
      name: "settings.toolUninstall",
    });
    expect(confirm).toBeDisabled();
    expect(dialog.queryByText(native.command)).toBeNull();

    fireEvent.click(npmRadio);
    expect(dialog.getByText(npm.command)).toBeInTheDocument();
    expect(dialog.queryByText("settings.toolUninstallNativeHint")).toBeNull();
    fireEvent.click(nativeRadio);
    expect(dialog.getByText(native.command)).toBeInTheDocument();
    expect(
      dialog.getByText("settings.toolUninstallNativeHint"),
    ).toBeInTheDocument();
    fireEvent.click(confirm);
    await waitFor(() =>
      expect(mocks.runToolLifecycleAction).toHaveBeenCalledWith(
        ["grok"],
        "uninstall",
        {},
        native,
      ),
    );
  });

  it("discards a cancelled preview instead of showing it for another tool", async () => {
    const first = deferred<ToolUninstallCandidate[]>();
    mocks.prepareToolUninstall.mockReturnValueOnce(first.promise);
    await renderApps();
    await openUninstall("Claude Code");
    fireEvent.click(
      within(screen.getByRole("dialog")).getByRole("button", {
        name: "common.cancel",
      }),
    );
    await openUninstall("Codex");
    const dialog = within(screen.getByRole("dialog"));
    expect(
      await dialog.findByText(uninstallPlan("codex").path),
    ).toBeInTheDocument();
    await act(async () => first.resolve([candidate(uninstallPlan("claude"))]));
    expect(dialog.queryByText(uninstallPlan("claude").path)).toBeNull();
    expect(mocks.runToolLifecycleAction).not.toHaveBeenCalled();
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

  it("confirms uninstall, preserves the running task across navigation and refreshes to install", async () => {
    const running = deferred<void>();
    mocks.runToolLifecycleAction.mockImplementation(
      async ([tool]: string[]) => {
        await running.promise;
        missing.add(tool);
      },
    );
    const view = await renderApps();
    const uninstall = () => rowMenu("Claude Code");
    await openUninstall("Claude Code");
    expect(mocks.runToolLifecycleAction).not.toHaveBeenCalled();
    fireEvent.click(
      within(screen.getByRole("dialog")).getByRole("button", {
        name: "common.cancel",
      }),
    );
    expect(mocks.runToolLifecycleAction).not.toHaveBeenCalled();
    await waitForOverlayTransition();
    await openUninstall("Claude Code");
    fireEvent.click(
      await within(screen.getByRole("dialog")).findByRole("button", {
        name: "settings.toolUninstall",
      }),
    );
    expect(mocks.runToolLifecycleAction).toHaveBeenCalledTimes(1);
    expect(mocks.runToolLifecycleAction).toHaveBeenCalledWith(
      ["claude"],
      "uninstall",
      {},
      uninstallPlan("claude"),
    );
    expect(uninstall()).toBeDisabled();
    expect(uninstall()).toHaveAttribute("aria-busy", "true");
    expect(updateButton("Claude Code")).toBeDisabled();
    expect(updateButton("Codex")).toBeEnabled();
    view.unmount();
    await renderApps();
    expect(uninstall()).toBeDisabled();
    expect(uninstall()).toHaveAttribute("aria-busy", "true");
    await act(async () => running.resolve());
    expect(
      card("Claude Code").queryByRole("button", {
        name: "appPage.moreActions",
      }),
    ).toBeNull();
    expect(
      card("Claude Code").getByRole("button", { name: "settings.toolInstall" }),
    ).toBeEnabled();
    expect(
      card("Claude Code").getByText("common.notInstalled"),
    ).toBeInTheDocument();
    expect(mocks.probeToolInstallations).toHaveBeenCalledWith(["claude"]);
    expect(mocks.success).toHaveBeenCalledOnce();
    expect(mocks.warning).not.toHaveBeenCalled();
  });

  it("keeps uninstall available for a broken installation", async () => {
    mocks.getToolVersions.mockImplementation(async (tools: string[]) =>
      tools.map((name) => ({
        name,
        version: null,
        latest_version: "2.0.0",
        error: "cannot run",
        installed_but_broken: true,
        env_type: "windows",
        wsl_distro: null,
      })),
    );
    await renderApps();
    expect(
      card("Claude Code").getByRole("button", {
        name: "appPage.moreActions",
      }),
    ).toBeEnabled();
    expect(
      card("Claude Code").queryByRole("button", {
        name: "settings.toolInstall",
      }),
    ).toBeNull();
  });

  it("reports uninstall failure and unlocks the tool for retry", async () => {
    mocks.runToolLifecycleAction.mockRejectedValueOnce(
      new Error("permission denied"),
    );
    const errorLog = vi.spyOn(console, "error").mockImplementation(() => {});
    try {
      await renderApps();
      await openUninstall("Claude Code");
      fireEvent.click(
        await within(screen.getByRole("dialog")).findByRole("button", {
          name: "settings.toolUninstall",
        }),
      );
      await waitFor(() =>
        expect(mocks.error).toHaveBeenCalledWith(
          "settings.toolUninstallFailed",
          {
            description: "permission denied",
            closeButton: true,
          },
        ),
      );
      expect(
        card("Claude Code").getByRole("button", {
          name: "appPage.moreActions",
        }),
      ).toBeEnabled();
      expect(updateButton("Claude Code")).toBeEnabled();
      expect(mocks.success).not.toHaveBeenCalled();
    } finally {
      errorLog.mockRestore();
    }
  });

  it.each(["rejected", "empty"])(
    "warns when uninstall succeeds but installation diagnostics are %s",
    async (result) => {
      const errorLog = vi.spyOn(console, "error").mockImplementation(() => {});
      try {
        await renderApps();
        mocks.runToolLifecycleAction.mockImplementationOnce(async () => {
          missing.add("claude");
          if (result === "rejected") {
            mocks.probeToolInstallations.mockRejectedValueOnce(
              new Error("diagnostics failed"),
            );
          } else {
            mocks.probeToolInstallations.mockResolvedValueOnce([]);
          }
        });
        await openUninstall("Claude Code");
        fireEvent.click(
          await within(screen.getByRole("dialog")).findByRole("button", {
            name: "settings.toolUninstall",
          }),
        );
        await waitFor(() =>
          expect(mocks.warning).toHaveBeenCalledWith(
            "settings.toolUninstallWarningTitle",
            {
              description: "settings.toolUninstallCheckFailed",
              closeButton: true,
            },
          ),
        );
        expect(mocks.error).not.toHaveBeenCalled();
        expect(mocks.success).not.toHaveBeenCalled();
        expect(
          card("Claude Code").getByRole("button", {
            name: "settings.toolInstall",
          }),
        ).toBeEnabled();
      } finally {
        errorLog.mockRestore();
      }
    },
  );

  it.each(["rejected", "empty"])(
    "warns when uninstall succeeds but the status refresh is %s",
    async (result) => {
      const errorLog = vi.spyOn(console, "error").mockImplementation(() => {});
      try {
        await renderApps();
        mocks.runToolLifecycleAction.mockImplementationOnce(async () => {
          if (result === "rejected") {
            mocks.getToolVersions.mockRejectedValueOnce(
              new Error("refresh failed"),
            );
          } else {
            mocks.getToolVersions.mockResolvedValueOnce([]);
          }
        });
        await openUninstall("Claude Code");
        fireEvent.click(
          await within(screen.getByRole("dialog")).findByRole("button", {
            name: "settings.toolUninstall",
          }),
        );
        await waitFor(() =>
          expect(mocks.warning).toHaveBeenCalledWith(
            "settings.toolUninstallWarningTitle",
            {
              description: "settings.toolUninstallCheckFailed",
              closeButton: true,
            },
          ),
        );
        expect(mocks.error).not.toHaveBeenCalled();
        expect(mocks.success).not.toHaveBeenCalled();
        expect(
          card("Claude Code").getByRole("button", {
            name: "appPage.moreActions",
          }),
        ).toBeEnabled();
      } finally {
        errorLog.mockRestore();
      }
    },
  );

  it.each([
    ["Windows NT 10.0", null, true],
    ["Windows NT 10.0", "Ubuntu", false],
    ["X11; Linux x86_64", null, false],
  ])(
    "warns about Hermes leftovers only on native Windows (%s, %s)",
    async (userAgent, distro, warn) => {
      const platform = vi
        .spyOn(navigator, "userAgent", "get")
        .mockReturnValue(userAgent);
      try {
        mocks.prepareToolUninstall.mockResolvedValueOnce([
          candidate({
            ...uninstallPlan("hermes"),
            command: "hermes uninstall --yes",
            wsl_distro: distro,
          }),
        ]);
        mocks.runToolLifecycleAction.mockImplementationOnce(async () => {
          missing.add("hermes");
        });
        await renderApps();
        await openUninstall("Hermes");
        const dialog = within(screen.getByRole("dialog"));
        const confirm = await dialog.findByRole("button", {
          name: "settings.toolUninstall",
        });
        expect(
          dialog.queryByText("settings.toolUninstallHermesWindowsHint") !==
            null,
        ).toBe(warn);
        expect(mocks.runToolLifecycleAction).not.toHaveBeenCalled();
        fireEvent.click(confirm);
        await waitFor(() =>
          expect(
            card("Hermes").getByRole("button", {
              name: "settings.toolInstall",
            }),
          ).toBeEnabled(),
        );
        if (warn) {
          expect(mocks.warning).toHaveBeenCalledWith(
            "settings.toolUninstallWarningTitle",
            {
              description: "settings.toolUninstallHermesWindowsHint",
              closeButton: true,
            },
          );
          expect(mocks.success).not.toHaveBeenCalled();
        } else {
          expect(mocks.warning).not.toHaveBeenCalled();
          expect(mocks.success).toHaveBeenCalledOnce();
        }
        expect(mocks.error).not.toHaveBeenCalled();
      } finally {
        platform.mockRestore();
      }
    },
  );

  it("reports the Official desktop copy after removing the last standalone CLI", async () => {
    const original = mocks.getToolVersions.getMockImplementation()!;
    const desktop = {
      path: "/Applications/ChatGPT.app",
      version: "26.1117.41037",
      bundled_cli_version: "0.171.0-alpha.13",
    };
    const plan = uninstallPlan("codex");
    const npm = candidate(plan, { source: "npm", version: "0.171.0" });
    mocks.getToolVersions.mockImplementation(async (tools: string[]) =>
      ((await original(tools)) as ToolVersions).map((version) =>
        version.name === "codex"
          ? {
              ...version,
              desktop_installation: desktop,
              desktop_managed: true,
              version: null,
              latest_version: null,
            }
          : version,
      ),
    );
    const reports = async (tools: string[]) =>
      tools.map((tool) =>
        report(tool, {
          installs: tool === "codex" && !upgraded.has(tool) ? [npm] : [],
        }),
      );
    mocks.getToolInstallationReports.mockImplementation(reports);
    mocks.probeToolInstallations.mockImplementation(reports);
    mocks.prepareToolUninstall.mockResolvedValueOnce([npm]);
    await renderApps();
    const app = card("Codex");
    const user = userEvent.setup();
    await user.click(
      await app.findByRole("button", { name: "appsPage.manageInstallations" }),
    );
    await user.click(
      await app.findByRole("button", { name: "settings.toolUninstall" }),
    );
    const dialog = within(await screen.findByRole("dialog"));
    expect(await dialog.findByText(npm.path)).toBeVisible();
    await user.click(
      dialog.getByRole("button", { name: "settings.toolUninstall" }),
    );
    await waitFor(() =>
      expect(mocks.info).toHaveBeenCalledWith(
        "settings.toolUninstallRemaining",
        { description: "Codex\nOfficial 26.1117.41037", closeButton: true },
      ),
    );
    expect(app.queryByText(npm.path)).toBeNull();
    expect(
      app.queryByRole("button", { name: "settings.toolUninstall" }),
    ).toBeNull();
    expect(app.getAllByText(desktop.version).length).toBeGreaterThan(0);
  });

  it("refreshes the remaining installation after uninstall instead of reporting a broken install", async () => {
    await renderApps();
    mocks.probeToolInstallations.mockResolvedValueOnce([
      report("claude", {
        installs: [
          {
            path: "C:/remaining/claude.exe",
            version: "2.0.0",
            runnable: true,
            error: null,
            source: "system",
            is_path_default: true,
          },
        ],
      }),
    ]);
    await openUninstall("Claude Code");
    fireEvent.click(
      await within(screen.getByRole("dialog")).findByRole("button", {
        name: "settings.toolUninstall",
      }),
    );
    await waitFor(() =>
      expect(mocks.info).toHaveBeenCalledWith(
        "settings.toolUninstallRemaining",
        {
          description: "Claude Code\nappsPage.installSourceUnknown 2.0.0",
          closeButton: true,
        },
      ),
    );
    expect(
      card("Claude Code").queryByText("C:/remaining/claude.exe"),
    ).toBeNull();
    fireEvent.click(
      await card("Claude Code").findByRole("button", {
        name: "appsPage.manageInstallations",
      }),
    );
    expect(
      await card("Claude Code").findByRole("button", {
        name: "settings.toolUninstall",
      }),
    ).toBeEnabled();
    expect(
      card("Claude Code").getByText("C:/remaining/claude.exe"),
    ).toBeInTheDocument();
    expect(mocks.warning).not.toHaveBeenCalled();
  });
});
