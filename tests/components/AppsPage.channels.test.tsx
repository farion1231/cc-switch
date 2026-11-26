import { vi, expect, it, describe } from "vitest";
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import type { settingsApi } from "@/lib/api/settings";
import { createTestQueryClient } from "../utils/testQueryClient";
import { waitForOverlayTransition } from "../utils/overlayTransition";
import {
  mocks,
  deferred,
  report,
  uninstallPlan,
  candidate,
  upgraded,
  outdated,
  card,
  updateButton,
  renderApps,
  mountApps,
  updateAllButton,
} from "./appsPageTestHelpers";
import type { ToolVersions } from "./appsPageTestHelpers";
import userEvent from "@testing-library/user-event";

describe("AppsPage installation channels", () => {
  it.each(["codex", "opencode"] as const)(
    "checks the lone standalone %s when a Windows desktop CLI is default",
    async (tool) => {
      outdated.clear();
      const original = mocks.getToolVersions.getMockImplementation()!;
      mocks.getToolVersions.mockImplementation(async (tools: string[]) =>
        ((await original(tools)) as ToolVersions).map((version) =>
          version.name === tool
            ? {
                ...version,
                desktop_managed: tool === "codex",
                desktop_installation: undefined,
                version: "1.0.0",
                latest_version: null,
                env_type: "windows",
              }
            : version,
        ),
      );
      const desktop = candidate(null, {
        source: "desktop",
        path: `/qa/desktop/${tool}`,
        is_path_default: true,
      });
      const standalone = candidate(null, {
        source: "npm",
        path: `/qa/npm/${tool}`,
        is_path_default: false,
        version: "1.0.0",
      });
      mocks.getToolInstallationReports.mockImplementation(
        async (tools: string[]) =>
          tools.map((name) =>
            report(name, {
              installs: name === tool ? [desktop, standalone] : [],
            }),
          ),
      );
      mocks.getToolInstallationUpdates.mockImplementation(
        async (name: string) =>
          name === tool
            ? [
                {
                  installation: standalone,
                  latest_version: "2.0.0",
                  plan: {
                    tool,
                    path: standalone.path,
                    real_path: standalone.path,
                    version: "1.0.0",
                    source: "npm",
                    command: "qa standalone update",
                  },
                },
              ]
            : [],
      );
      await renderApps();
      await waitFor(() =>
        expect(mocks.getToolInstallationUpdates).toHaveBeenCalledWith(
          tool,
          undefined,
          true,
        ),
      );
      await waitFor(() => expect(updateAllButton()).toBeEnabled());
    },
  );

  it("lets the only remaining CLI clear an unavailable default", async () => {
    outdated.clear();
    const working = candidate(null, {
      source: "homebrew",
      path: "/qa/brew/bin/opencode",
      version: "2.0.0",
    });
    mocks.getToolInstallationReports.mockImplementation(
      async (tools: string[]) =>
        tools.map((tool) =>
          report(tool, { installs: tool === "opencode" ? [working] : [] }),
        ),
    );
    mocks.getPreferredToolInstallation.mockImplementation(async (tool) => ({
      supported: true,
      path: tool === "opencode" ? "/removed/npm/bin/opencode" : null,
      disabled_paths: [],
      selected_path: tool === "opencode" ? working.path : null,
      selected_real_path: null,
      selection: "automatic",
    }));
    await renderApps();
    const app = card("OpenCode");
    await waitFor(() => expect(app.getByText("Homebrew")).toBeInTheDocument());
    const selector = await app.findByRole("combobox");
    expect(selector).toHaveTextContent("appsPage.cliDefaultUnavailable");
    expect(app.getByText("appsPage.cliUsingAutomatic")).toBeInTheDocument();
    const user = userEvent.setup();
    await user.click(selector);
    await user.click(
      await screen.findByRole("option", { name: "common.auto" }),
    );
    expect(mocks.setPreferredToolInstallation).toHaveBeenCalledWith(
      "opencode",
      null,
      undefined,
    );
    await waitFor(() => expect(app.queryByRole("combobox")).toBeNull());
  });

  it("confirms and upgrades each independent Codex CLI without touching Official", async () => {
    outdated.clear();
    const original = mocks.getToolVersions.getMockImplementation()!;
    mocks.getToolVersions.mockImplementation(async (tools: string[]) =>
      ((await original(tools)) as ToolVersions).map((version) =>
        version.name === "codex"
          ? {
              ...version,
              desktop_installation: {
                path: "/Applications/ChatGPT.app",
                version: "26.1.1",
              },
              desktop_managed: true,
              version: null,
              latest_version: null,
            }
          : version,
      ),
    );
    const npm = candidate(null, {
      source: "npm",
      path: "/qa/npm/bin/codex",
      version: "1.0.0",
    });
    const plan = {
      tool: "codex",
      path: npm.path,
      real_path: npm.path,
      version: npm.version,
      source: "npm",
      command: "qa update command",
    };
    const curl = candidate(null, {
      source: "curl",
      path: "/qa/curl/bin/codex",
      version: "1.0.0",
      is_path_default: false,
    });
    const curlPlan = {
      ...plan,
      path: curl.path,
      real_path: curl.path,
      source: "curl",
      command: "qa curl update command",
    };
    const official = candidate(null, {
      source: "desktop",
      path: "/Applications/ChatGPT.app/Contents/Resources/codex",
      version: "1.0.0",
    });
    const refreshed = new Set<string>();
    const reports = async (tools: string[]) =>
      tools.map((tool) =>
        report(tool, {
          installs:
            tool === "codex"
              ? [
                  official,
                  ...[npm, curl].map((copy) =>
                    refreshed.has(copy.path)
                      ? { ...copy, version: "2.0.0" }
                      : copy,
                  ),
                ]
              : [],
        }),
      );
    mocks.getToolInstallationReports.mockImplementation(reports);
    mocks.probeToolInstallations.mockImplementation(reports);
    mocks.getToolInstallationUpdates.mockImplementation(async (tool) =>
      tool === "codex"
        ? [
            {
              installation: official,
              latest_version: "2.0.0",
              plan: { ...plan, path: official.path },
            },
            ...[plan, curlPlan].map((update) => ({
              installation: {
                ...(update === plan ? npm : curl),
                version: refreshed.has(update.path) ? "2.0.0" : "1.0.0",
              },
              latest_version: "2.0.0",
              plan: update,
            })),
          ]
        : [],
    );
    const first = deferred<void>();
    mocks.runToolLifecycleAction.mockImplementation(async (...args) => {
      const planned = args[6] as typeof plan;
      if (planned.path === npm.path) await first.promise;
      refreshed.add(planned.path);
    });
    await renderApps();
    const user = userEvent.setup();
    await user.click(
      await screen.findByRole("button", { name: /settings.updateAllTools/ }),
    );
    const confirm = () =>
      within(screen.getByRole("dialog")).getByRole("button", {
        name: "settings.toolUpgradeConfirmBtn",
      });
    const dialog = within(await screen.findByRole("dialog"));
    expect(dialog.getByText(plan.command)).toBeInTheDocument();
    expect(dialog.getByText(curlPlan.command)).toBeInTheDocument();
    expect(dialog.getAllByText("appsPage.updateTarget")).toHaveLength(2);
    expect(mocks.runToolLifecycleAction).not.toHaveBeenCalled();
    await user.click(dialog.getByRole("button", { name: "common.cancel" }));
    expect(mocks.runToolLifecycleAction).not.toHaveBeenCalled();
    await waitForOverlayTransition();
    await user.click(updateAllButton());
    await user.click(confirm());
    expect(mocks.runToolLifecycleAction).toHaveBeenCalledTimes(1);
    expect(mocks.runToolLifecycleAction.mock.calls[0]?.[6]).toEqual(plan);
    await act(async () => first.resolve());
    await waitFor(() =>
      expect(
        mocks.runToolLifecycleAction.mock.calls.map((args) => args[6]),
      ).toEqual([plan, curlPlan]),
    );
    await waitFor(() =>
      expect(
        screen.queryByRole("button", { name: /settings.updateAllTools/ }),
      ).toBeNull(),
    );
  });
  it("shows the primary update once in the multi-copy version column", async () => {
    outdated.clear();
    outdated.add("opencode");
    const brew = candidate(null, {
      source: "homebrew",
      path: "/qa/brew/bin/opencode",
      version: "1.0.0",
    });
    const curl = candidate(null, {
      source: "curl",
      path: "/qa/curl/bin/opencode",
      version: "2.0.0",
      is_path_default: false,
    });
    mocks.getToolInstallationReports.mockImplementation(
      async (tools: string[]) =>
        tools.map((tool) =>
          report(tool, { installs: tool === "opencode" ? [brew, curl] : [] }),
        ),
    );
    mocks.getToolInstallationUpdates.mockImplementation(async (tool) =>
      tool === "opencode"
        ? [
            { installation: brew, latest_version: "2.0.0" },
            { installation: curl, latest_version: "2.0.0" },
          ]
        : [],
    );
    await renderApps();
    const app = card("OpenCode");
    const versions = within(
      await app.findByRole("list", { name: "appsPage.columnVersion" }),
    );
    await waitFor(() => {
      const notice = versions.getByText("appsPage.newVersion");
      expect(notice.closest("li")).toHaveTextContent("Homebrew");
    });
  });

  it.each([
    "mixed preview",
    "preflight failure",
    "partial execution",
    "primary Node gap",
  ])("keeps batch upgrade consistent for %s", async (scenario) => {
    outdated.clear();
    if (scenario === "mixed preview" || scenario === "preflight failure")
      outdated.add("gemini");
    if (scenario === "preflight failure") outdated.add("claude");
    const original = mocks.getToolVersions.getMockImplementation()!;
    mocks.getToolVersions.mockImplementation(async (tools: string[]) =>
      ((await original(tools)) as ToolVersions).map((version) =>
        version.name === "codex" && scenario === "primary Node gap"
          ? {
              ...version,
              version: "1.0.0",
              latest_version: "2.0.0",
              update_blocked_by_node: { required: ">=24", current: "22" },
            }
          : version.name === "codex"
            ? {
                ...version,
                desktop_installation: {
                  path: "/Applications/ChatGPT.app",
                  version: "26.1.1",
                },
                desktop_managed: true,
                version: null,
                latest_version: null,
              }
            : version,
      ),
    );
    const npm = candidate(null, {
      source: "npm",
      path: "/qa/npm/bin/codex",
      version: "1.0.0",
    });
    const plan = {
      tool: "codex",
      path: npm.path,
      real_path: npm.path,
      version: npm.version,
      source: "npm",
      command: "qa update command",
    };
    const curl = candidate(null, {
      source: "curl",
      path: "/qa/curl/bin/codex",
      version: "1.0.0",
      is_path_default: false,
    });
    const curlPlan = {
      ...plan,
      path: curl.path,
      real_path: curl.path,
      source: "curl",
      command: "qa curl update command",
    };
    const official = candidate(null, {
      source: "desktop",
      path: "/Applications/ChatGPT.app/Contents/Resources/codex",
      version: "1.0.0",
    });
    const refreshed = new Set<string>();
    const reports = async (tools: string[]) =>
      tools.map((tool) =>
        report(tool, {
          installs:
            tool === "codex"
              ? [
                  official,
                  ...[npm, curl].map((copy) =>
                    refreshed.has(copy.path)
                      ? { ...copy, version: "2.0.0" }
                      : copy,
                  ),
                ]
              : [],
        }),
      );
    mocks.getToolInstallationReports.mockImplementation(reports);
    mocks.probeToolInstallations.mockImplementation(reports);
    mocks.getToolInstallationUpdates.mockImplementation(async (tool) =>
      tool === "codex"
        ? [
            {
              installation: official,
              latest_version: "2.0.0",
              plan: { ...plan, path: official.path },
            },
            ...[plan, curlPlan].map((update) => ({
              installation: {
                ...(update === plan ? npm : curl),
                version: refreshed.has(update.path) ? "2.0.0" : "1.0.0",
              },
              latest_version: "2.0.0",
              update_blocked_by_node:
                scenario === "primary Node gap" && update === plan
                  ? { required: ">=24", current: "22" }
                  : null,
              plan: update,
            })),
          ]
        : [],
    );

    const gemini = candidate(null, {
      source: "npm",
      path: "/qa/npm/bin/gemini",
      version: "1.0.0",
    });
    const geminiPlan = {
      ...plan,
      tool: "gemini",
      path: gemini.path,
      real_path: gemini.path,
      command: "qa Gemini upgrade",
    };
    mocks.probeToolInstallations.mockImplementation(async (tools: string[]) => {
      if (scenario === "preflight failure" && tools.includes("gemini")) {
        throw new Error("Gemini preflight unavailable");
      }
      return (await reports(tools)).map((item) =>
        item.tool === "gemini"
          ? {
              ...item,
              installs: [gemini],
              command: geminiPlan.command,
              update_plan: geminiPlan,
            }
          : item,
      );
    });
    mocks.runToolLifecycleAction.mockImplementation(async (...args) => {
      const planned = args[6] as typeof plan | undefined;
      if (!planned) {
        upgraded.add((args[0] as string[])[0]);
        return;
      }
      if (scenario === "partial execution" && planned.path === npm.path) {
        throw new Error("Confirmed installation changed");
      }
      refreshed.add(planned.path);
      upgraded.add(planned.tool);
    });
    await renderApps();
    if (scenario === "primary Node gap") {
      const codex = card("Codex");
      await userEvent.click(
        await codex.findByRole("button", {
          name: "appsPage.manageInstallations",
        }),
      );
      const details = within(
        codex.getByRole("region", { name: "appsPage.inspectInstallations" }),
      );
      await waitFor(() => {
        const curlRow = within(details.getByText(curl.path).closest("li")!);
        expect(
          curlRow.getByRole("button", { name: "settings.toolUpdate" }),
        ).toBeEnabled();
      });
      const npmRow = within(details.getByText(npm.path).closest("li")!);
      expect(npmRow.getByText("appsPage.updateNeedsNode")).toBeInTheDocument();
      expect(
        npmRow.queryByRole("button", { name: "settings.toolUpdate" }),
      ).toBeNull();
      await userEvent.click(updateAllButton());
      const dialog = within(await screen.findByRole("dialog"));
      expect(dialog.getByText(curlPlan.command)).toBeInTheDocument();
      expect(dialog.queryByText(plan.command)).toBeNull();
      await userEvent.click(
        dialog.getByRole("button", { name: "settings.toolUpgradeConfirmBtn" }),
      );
      await waitFor(() =>
        expect(mocks.runToolLifecycleAction).toHaveBeenCalledTimes(1),
      );
      expect(mocks.runToolLifecycleAction.mock.calls[0]?.[6]).toEqual(curlPlan);
      return;
    }
    await waitFor(() => {
      const update = updateAllButton();
      expect(update).toBeEnabled();
      expect(card("Codex").getAllByText("appsPage.newVersion")).toHaveLength(2);
    });
    await userEvent.click(updateAllButton());
    const dialog = within(await screen.findByRole("dialog"));
    expect(dialog.getByText(plan.command)).toBeInTheDocument();
    expect(dialog.getByText(curlPlan.command)).toBeInTheDocument();
    if (scenario === "mixed preview") {
      expect(dialog.getByText(geminiPlan.command)).toBeInTheDocument();
      expect(dialog.getByText(gemini.path)).toBeInTheDocument();
    } else {
      expect(dialog.queryByText(geminiPlan.command)).toBeNull();
    }
    if (scenario === "preflight failure") {
      expect(dialog.getByText("Claude Code")).toBeInTheDocument();
      expect(mocks.probeToolInstallations).toHaveBeenCalledWith(["claude"]);
      expect(mocks.probeToolInstallations).toHaveBeenCalledWith(["gemini"]);
      expect(mocks.warning).toHaveBeenCalledWith(
        "settings.toolDiagnoseFailed",
        {
          description: "Gemini CLI",
          closeButton: true,
        },
      );
    }
    expect(mocks.runToolLifecycleAction).not.toHaveBeenCalled();
    await userEvent.click(
      dialog.getByRole("button", { name: "settings.toolUpgradeConfirmBtn" }),
    );
    await waitFor(() =>
      expect(mocks.runToolLifecycleAction).toHaveBeenCalledWith(
        ["codex"],
        "update",
        expect.anything(),
        undefined,
        undefined,
        undefined,
        curlPlan,
      ),
    );
    if (scenario === "mixed preview") {
      expect(mocks.runToolLifecycleAction).toHaveBeenCalledWith(
        ["gemini"],
        "update",
        expect.anything(),
        undefined,
        undefined,
        undefined,
        geminiPlan,
      );
    } else if (scenario === "preflight failure") {
      const actions = mocks.runToolLifecycleAction.mock.calls.flatMap(
        ([tools]) => tools,
      );
      expect(actions).toContain("claude");
      expect(actions).not.toContain("gemini");
    } else {
      await waitFor(() =>
        expect(mocks.warning).toHaveBeenCalledWith(
          "settings.toolActionPartial",
          expect.anything(),
        ),
      );
      expect(refreshed.has(curl.path)).toBe(true);
      expect(refreshed.has(npm.path)).toBe(false);
    }
  });

  it("prefers the official Codex desktop package over the independent npm CLI", async () => {
    outdated.clear();
    outdated.add("codex");
    const original = mocks.getToolVersions.getMockImplementation()!;
    const desktop = {
      path: "/Applications/ChatGPT.app",
      version: "26.1117.31558",
      bundled_cli_version: "0.171.0-alpha.13",
    };
    let desktopInstalled = true;
    mocks.getToolVersions.mockImplementation(async (tools: string[]) =>
      ((await original(tools)) as ToolVersions).map((version) =>
        version.name === "codex" && desktopInstalled
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
    mocks.probeToolInstallations.mockImplementation(async (tools: string[]) =>
      tools.map((tool) =>
        report(tool, {
          installs:
            tool === "codex"
              ? [
                  {
                    path: "/opt/homebrew/bin/codex",
                    version: null,
                    runnable: false,
                    error: "Missing optional dependency",
                    source: "npm",
                    is_path_default: true,
                  },
                ]
              : [],
        }),
      ),
    );

    await renderApps();
    expect(card("Codex").getAllByText("Official").length).toBeGreaterThan(0);
    expect(card("Codex").getByText(desktop.version)).toBeInTheDocument();
    expect(card("Codex").queryByText(desktop.path)).toBeNull();
    expect(card("Codex").queryByText("appsPage.notRunnable")).toBeNull();
    expect(
      screen.queryByRole("button", { name: /settings\.updateAllTools/ }),
    ).toBeNull();

    const user = userEvent.setup();
    await user.click(
      screen.getByRole("button", { name: "appsPage.moreActions" }),
    );
    await user.click(
      await screen.findByRole("menuitem", { name: "settings.toolDiagnose" }),
    );
    expect(
      await card("Codex").findByText("Missing optional dependency"),
    ).toBeInTheDocument();
    expect(card("Codex").getByText(desktop.path)).toBeInTheDocument();
    expect(
      card("Codex").getByText("/opt/homebrew/bin/codex"),
    ).toBeInTheDocument();
    expect(card("Codex").getAllByText("npm").length).toBeGreaterThan(0);

    desktopInstalled = false;
    await user.click(
      screen.getByRole("button", { name: "appsPage.checkUpdates" }),
    );
    await waitFor(() => expect(updateButton("Codex")).toBeEnabled());
    expect(card("Codex").getByText("1.0.0")).toBeInTheDocument();
    expect(card("Codex").queryByText("Official")).toBeNull();
  });

  it("shows the desktop and terminal roles and pins repair to the selected npm installation", async () => {
    const original = mocks.getToolVersions.getMockImplementation()!;
    mocks.getToolVersions.mockImplementation(async (tools: string[]) =>
      ((await original(tools)) as ToolVersions).map((info) =>
        info.name === "codex"
          ? {
              ...info,
              version: null,
              latest_version: null,
              desktop_managed: true,
              desktop_installation: {
                path: "/Applications/ChatGPT.app",
                version: "26.1117.31558",
                bundled_cli_version: "0.171.0-alpha.13",
              },
            }
          : info,
      ),
    );
    const plan = {
      tool: "codex",
      path: "/opt/homebrew/bin/codex",
      real_path: "/opt/homebrew/lib/node_modules/@openai/codex/bin/codex.js",
      version: null,
      source: "npm",
      command:
        "/opt/homebrew/bin/npm install -g --prefix /opt/homebrew @openai/codex@latest",
    };
    mocks.getToolInstallationUpdates.mockResolvedValue([
      {
        installation: {
          path: plan.path,
          version: null,
          runnable: false,
          error: "Missing dependency",
          source: "npm",
          is_path_default: true,
        },
        package_version: "0.170.0",
        latest_version: null,
        plan,
      },
    ]);

    await renderApps();
    const codex = card("Codex");
    const user = userEvent.setup();
    await user.click(
      codex.getByRole("button", { name: "appsPage.manageInstallations" }),
    );
    expect(
      await codex.findByText("appsPage.desktopSelfUpdates"),
    ).toBeInTheDocument();
    expect(
      within(
        codex.getByRole("region", { name: "appsPage.inspectInstallations" }),
      ).getByText("appsPage.bundledCliVersion"),
    ).toBeInTheDocument();
    expect(codex.queryByText("appsPage.pathDefault")).toBeNull();
    expect(codex.queryByText("appsPage.shownInSummary")).toBeNull();
    expect(
      within(
        codex.getByRole("region", { name: "appsPage.inspectInstallations" }),
      ).getByText("0.170.0"),
    ).toBeInTheDocument();
    expect(codex.getAllByText("Official").length).toBeGreaterThan(0);
    expect(codex.getAllByText("npm").length).toBeGreaterThan(0);
    await user.click(
      codex.getByRole("button", { name: "appsPage.repairInstallation" }),
    );

    const dialog = within(await screen.findByRole("dialog"));
    expect(dialog.getByText("appsPage.updateTarget")).toBeInTheDocument();
    expect(dialog.getByText(plan.command)).toBeInTheDocument();
    expect(mocks.runToolLifecycleAction).not.toHaveBeenCalled();
    await user.click(
      dialog.getByRole("button", { name: "settings.toolUpgradeConfirmBtn" }),
    );
    await waitFor(() =>
      expect(mocks.runToolLifecycleAction).toHaveBeenCalledWith(
        ["codex"],
        "update",
        expect.any(Object),
        undefined,
        undefined,
        undefined,
        plan,
      ),
    );
  });

  it("keeps the Official row clear when using the bundled CLI", async () => {
    const getVersions = mocks.getToolVersions.getMockImplementation()!;
    mocks.getToolVersions.mockImplementation(async (tools: string[]) =>
      ((await getVersions(tools)) as ToolVersions).map((tool) =>
        tool.name === "codex"
          ? {
              ...tool,
              version: null,
              desktop_managed: true,
              desktop_installation: {
                path: "/Applications/ChatGPT.app",
                version: "26.1117.41037",
              },
            }
          : tool,
      ),
    );
    mocks.getPreferredToolInstallation.mockResolvedValue({
      supported: true,
      path: null,
      disabled_paths: [],
      selected_path:
        "/Applications/ChatGPT.app/Contents/Resources/codex-cli/bin/codex",
      selected_real_path: null,
      selection: "bundled",
    });
    await renderApps();
    const codex = card("Codex");
    expect(codex.getByText("26.1117.41037")).toBeInTheDocument();
    await waitFor(() =>
      expect(mocks.getPreferredToolInstallation).toHaveBeenCalledWith("codex"),
    );
    expect(codex.queryByRole("status")).toBeNull();
    expect(codex.queryByText("appsPage.cliSelectionUnavailable")).toBeNull();
    expect(
      codex.queryByRole("combobox", { name: "appsPage.cliVersionChoice" }),
    ).toBeNull();
  });

  it("lets Codex choose between curl and its bundled CLI, and falls back to Official", async () => {
    const desktop = "/Applications/ChatGPT.app";
    const bundled = `${desktop}/Contents/Resources/codex-cli/bin/codex`;
    const curl = {
      ...candidate(uninstallPlan("codex"), { source: "curl" }),
      path: "/qa/.local/bin/codex",
      version: "0.171.0",
    };
    const getVersions = mocks.getToolVersions.getMockImplementation()!;
    mocks.getToolVersions.mockImplementation(async (tools: string[]) =>
      ((await getVersions(tools)) as ToolVersions).map((tool) =>
        tool.name === "codex"
          ? {
              ...tool,
              desktop_managed: true,
              desktop_installation: {
                path: desktop,
                version: "26.1117.41037",
                bundled_cli_version: "0.171.0-alpha.13",
              },
            }
          : tool,
      ),
    );
    mocks.getToolInstallationReports.mockImplementation(
      async (tools: string[]) =>
        tools.map((tool) =>
          report(tool, { installs: tool === "codex" ? [curl] : [] }),
        ),
    );
    const updatePlan = {
      tool: "codex",
      path: curl.path,
      real_path: curl.path,
      version: curl.version,
      source: "curl",
      command: "update the reviewed curl installation",
    };
    mocks.getToolInstallationUpdates.mockImplementation(
      async (tool, _path, includeLatest) =>
        tool === "codex" && includeLatest
          ? [
              {
                installation: curl,
                latest_version: "0.172.0",
                plan: updatePlan,
              },
            ]
          : [],
    );
    let preference: string | null = null;
    let curlDisabled = false;
    const policy = () => ({
      supported: true,
      bundled_available: true,
      path: preference,
      disabled_paths: curlDisabled ? [curl.path] : [],
      selected_path:
        preference === "bundled" || curlDisabled ? bundled : curl.path,
      selected_real_path: null,
      selection: preference
        ? "explicit"
        : curlDisabled
          ? "bundled"
          : "automatic",
    });
    mocks.getPreferredToolInstallation.mockImplementation(async () => policy());
    mocks.setPreferredToolInstallation.mockImplementation(
      async (_tool, path, enabled) => {
        if (enabled === undefined) preference = path;
        else if (path === curl.path) curlDisabled = !enabled;
        return policy();
      },
    );

    await renderApps();
    const codex = card("Codex");
    const user = userEvent.setup();
    expect(await codex.findByText("appsPage.newVersion")).toBeInTheDocument();
    expect(mocks.getToolInstallationUpdates).toHaveBeenCalledWith(
      "codex",
      undefined,
      true,
    );
    const picker = await codex.findByRole("combobox", {
      name: "appsPage.cliVersionChoice",
    });
    const choose = async (name: string) => {
      await user.click(picker);
      await user.click(await screen.findByRole("option", { name }));
    };
    await choose("Official");
    expect(mocks.setPreferredToolInstallation).toHaveBeenLastCalledWith(
      "codex",
      "bundled",
      undefined,
    );
    expect(picker).toHaveTextContent(/^Official$/);
    await choose("curl");
    expect(mocks.setPreferredToolInstallation).toHaveBeenLastCalledWith(
      "codex",
      curl.path,
      undefined,
    );
    expect(picker).toHaveTextContent(/^curl$/);
    await choose("common.auto");
    await user.click(
      codex.getByRole("button", { name: "appsPage.manageInstallations" }),
    );
    expect(
      codex.getByRole("button", { name: "settings.toolUpdate" }),
    ).toBeEnabled();
    const curlSwitch = await codex.findByRole("switch", {
      name: "appsPage.cliEnable · curl",
    });
    await user.click(curlSwitch);
    expect(curlSwitch).not.toBeChecked();
    expect(picker).toHaveTextContent(/^common\.auto$/);
    const badge = within(codex.getByText("Codex").parentElement!);
    expect(await badge.findByText("Official")).toBeInTheDocument();
    expect(codex.getByText("appsPage.cliUsingAutomatic")).toBeInTheDocument();
    expect(codex.queryByText("appsPage.cliSelectionUnavailable")).toBeNull();
    expect(
      codex.queryByRole("switch", { name: "appsPage.cliEnable · Official" }),
    ).toBeNull();
    await user.click(picker);
    expect(
      await screen.findByRole("option", { name: "Official" }),
    ).toBeInTheDocument();
    expect(screen.queryByRole("option", { name: "curl" })).toBeNull();
  });

  it("shows updates for secondary copies and resolves disabled launcher aliases", async () => {
    const primary = candidate(null, {
      source: "homebrew",
      path: "/qa/brew/bin/opencode",
      version: "2.0.0",
    });
    const secondary = candidate(null, {
      source: "curl",
      path: "/qa/alias/bin/opencode",
      real_path: "/qa/install/opencode",
      version: "1.0.0",
      is_path_default: false,
    });
    const plan = {
      tool: "opencode",
      path: secondary.path,
      real_path: secondary.real_path,
      version: secondary.version,
      source: "curl",
      command: "update reviewed secondary installation",
    };
    mocks.getToolInstallationReports.mockImplementation(
      async (tools: string[]) =>
        tools.map((tool) =>
          report(tool, {
            installs: tool === "opencode" ? [primary, secondary] : [],
          }),
        ),
    );
    mocks.getToolInstallationUpdates.mockImplementation(
      async (tool, _path, includeLatest) =>
        tool === "opencode" && includeLatest
          ? [
              { installation: primary, latest_version: "2.0.0" },
              { installation: secondary, latest_version: "2.0.0", plan },
            ]
          : [],
    );
    mocks.getPreferredToolInstallation.mockResolvedValue({
      supported: true,
      path: null,
      disabled_paths: ["/qa/old/opencode"],
      disabled_real_paths: [secondary.real_path],
      selected_path: primary.path,
      selected_real_path: null,
      selection: "automatic",
    });
    await renderApps();
    const app = card("OpenCode");
    expect(await app.findByText("appsPage.newVersion")).toBeInTheDocument();
    const picker = await app.findByRole("combobox", {
      name: "appsPage.cliVersionChoice",
    });
    const user = userEvent.setup();
    await user.click(picker);
    expect(
      await screen.findByRole("option", { name: "Homebrew" }),
    ).toBeInTheDocument();
    expect(screen.queryByRole("option", { name: "curl" })).toBeNull();
    await user.keyboard("{Escape}");
    await user.click(
      app.getByRole("button", { name: "appsPage.manageInstallations" }),
    );
    const details = app.getByRole("region", {
      name: "appsPage.inspectInstallations",
    });
    expect(
      within(details).getByRole("button", { name: "settings.toolUpdate" }),
    ).toBeEnabled();
    expect(
      within(details).getByRole("switch", {
        name: "appsPage.cliEnable · curl",
      }),
    ).not.toBeChecked();
  });

  it("uninstalls only the path selected in the installation list", async () => {
    const brew = {
      ...uninstallPlan("codex"),
      path: "/opt/homebrew/bin/codex",
      real_path: "/opt/homebrew/Cellar/codex/1.0.0/bin/codex",
      command: "brew uninstall --formula codex",
    };
    const npm = {
      ...uninstallPlan("codex"),
      path: "/usr/local/bin/codex",
      real_path: "/usr/local/lib/node_modules/@openai/codex/bin/codex.js",
    };
    const brewInstall = candidate(brew, { source: "homebrew" });
    const npmInstall = candidate(npm, {
      source: "npm",
      version: "1.1.0",
      is_path_default: false,
    });
    mocks.getToolInstallationReports.mockImplementation(
      async (tools: string[]) =>
        tools.map((tool) =>
          report(tool, {
            installs: tool === "codex" ? [brewInstall, npmInstall] : [],
          }),
        ),
    );
    const updates = [brewInstall, npmInstall].map((installation) => ({
      installation,
      real_path: installation.plan!.real_path,
      package_version: null,
      latest_version: null,
      plan: {
        tool: "codex",
        path: installation.path,
        real_path: installation.plan!.real_path,
        version: installation.version,
        source: installation.source,
        command: "update",
      },
    }));
    mocks.getToolInstallationUpdates.mockImplementation(async (_tool, path) =>
      path
        ? updates
            .filter((entry) => entry.installation.path === path)
            .map((entry) => ({
              ...entry,
              latest_version: "1.2.0",
            }))
        : updates,
    );
    mocks.prepareToolUninstall
      .mockResolvedValueOnce([brewInstall])
      .mockResolvedValueOnce([brewInstall, npmInstall]);

    let preference: string | null = null;
    const disabled = new Set<string>();
    const getPolicy = () => {
      const selection =
        preference ??
        [brew.path, npm.path].find((path) => !disabled.has(path)) ??
        null;
      return {
        supported: true,
        path: preference,
        disabled_paths: [...disabled],
        selected_path: selection,
        selected_real_path: null,
        selection: !selection
          ? "disabled"
          : preference
            ? "explicit"
            : "automatic",
      };
    };
    mocks.getPreferredToolInstallation.mockImplementation(async () =>
      getPolicy(),
    );
    mocks.setPreferredToolInstallation.mockImplementation(
      async (_tool, path, enabled) => {
        if (path && enabled === false) {
          disabled.add(path);
          if (preference === path) preference = null;
        } else if (path && enabled === true) {
          disabled.delete(path);
        } else {
          preference = path;
        }
        return getPolicy();
      },
    );

    await renderApps();
    const codex = card("Codex");
    const versions = await codex.findByRole("list", {
      name: "appsPage.columnVersion",
    });
    expect(versions).toHaveTextContent(/Homebrew\s*1\.0\.0/);
    expect(versions).toHaveTextContent(/npm\s*1\.1\.0/);
    expect(codex.queryByText(npm.path)).toBeNull();
    const user = userEvent.setup();
    expect(
      await codex.findByText("appsPage.cliUsingAutomatic"),
    ).toBeInTheDocument();
    const nameAndBadge = within(codex.getByText("Codex").parentElement!);
    expect(nameAndBadge.getByText("Homebrew")).toBeInTheDocument();
    expect(nameAndBadge.queryByText("npm")).toBeNull();
    expect(within(versions).queryByRole("switch")).toBeNull();
    const chooseCLI = async (name: string) => {
      await user.click(
        codex.getByRole("combobox", { name: "appsPage.cliVersionChoice" }),
      );
      await user.click(await screen.findByRole("option", { name }));
    };
    await chooseCLI("npm");
    expect(mocks.setPreferredToolInstallation).toHaveBeenCalledWith(
      "codex",
      npm.path,
      undefined,
    );
    expect(await nameAndBadge.findByText("npm")).toHaveAttribute(
      "title",
      "appsPage.cliPreferred",
    );
    expect(nameAndBadge.queryByText("Homebrew")).toBeNull();
    expect(codex.queryByRole("status")).toBeNull();
    expect(
      codex.getByRole("combobox", { name: "appsPage.cliVersionChoice" }),
    ).toHaveTextContent(/^npm$/);
    expect(
      codex.queryByRole("region", { name: "appsPage.inspectInstallations" }),
    ).toBeNull();

    await user.click(
      codex.getByRole("button", { name: "appsPage.manageInstallations" }),
    );
    const details = codex.getByRole("region", {
      name: "appsPage.inspectInstallations",
    });
    const npmRow = within(details).getByText(npm.path).closest("li")!;
    const brewRow = within(details).getByText(brew.path).closest("li")!;
    const npmActions = within(npmRow);
    const switchFor = (row: Element) =>
      within(row as HTMLElement).getByRole("switch", {
        name: /^appsPage\.cliEnable · /,
      });
    expect(switchFor(npmRow)).toHaveAccessibleName("appsPage.cliEnable · npm");
    expect(switchFor(brewRow)).toHaveAccessibleName(
      "appsPage.cliEnable · Homebrew",
    );
    expect(npmActions.getByText("appsPage.cliEnable")).toBeVisible();
    expect(npmActions.getByText("1.1.0")).toBeInTheDocument();
    expect(npmActions.getByText("appsPage.cliPreferred")).toBeInTheDocument();
    await user.click(switchFor(npmRow));
    expect(mocks.setPreferredToolInstallation).toHaveBeenCalledWith(
      "codex",
      npm.path,
      false,
    );
    expect(
      await codex.findByText("appsPage.cliUsingAutomatic"),
    ).toBeInTheDocument();
    expect(npmActions.getByText("appsPage.cliDisabled")).toBeInTheDocument();
    await user.click(switchFor(brewRow));
    expect(
      await codex.findByText("appsPage.cliAllDisabled"),
    ).toBeInTheDocument();
    await user.click(switchFor(npmRow));
    expect(
      await codex.findByText("appsPage.cliUsingAutomatic"),
    ).toBeInTheDocument();
    expect(nameAndBadge.getByText("npm")).toBeInTheDocument();
    expect(nameAndBadge.queryByText("Homebrew")).toBeNull();
    await user.click(switchFor(brewRow));
    await chooseCLI("npm");
    await chooseCLI("common.auto");
    expect(mocks.setPreferredToolInstallation).toHaveBeenCalledWith(
      "codex",
      null,
      undefined,
    );
    const openSelected = () =>
      user.click(
        npmActions.getByRole("button", { name: "settings.toolUninstall" }),
      );
    await openSelected();
    let dialog = within(await screen.findByRole("dialog"));
    expect(
      await dialog.findByText("settings.toolUninstallManualRequired"),
    ).toBeInTheDocument();
    expect(
      dialog.queryByRole("button", { name: "settings.toolUninstall" }),
    ).toBeNull();
    expect(dialog.queryByText(brew.path)).toBeNull();
    await user.click(dialog.getByRole("button", { name: "common.cancel" }));

    await openSelected();
    dialog = within(await screen.findByRole("dialog"));
    expect(await dialog.findByText(npm.path)).toBeVisible();
    expect(dialog.queryByText(brew.path)).toBeNull();
    expect(dialog.queryByRole("radio")).toBeNull();
    expect(mocks.runToolLifecycleAction).not.toHaveBeenCalled();
    await user.click(
      dialog.getByRole("button", { name: "settings.toolUninstall" }),
    );
    expect(mocks.runToolLifecycleAction).toHaveBeenCalledWith(
      ["codex"],
      "uninstall",
      {},
      npm,
    );
  });

  it("keeps a mise copy distinct from npm across the Apps summary and details", async () => {
    const installs = [
      candidate(null, {
        path: "/qa/mise/installs/opencode/2.3.2/bin/opencode",
        source: "mise",
        version: "2.3.2",
      }),
      candidate(null, {
        path: "/qa/npm/bin/opencode",
        source: "npm",
        version: "1.19.6",
        is_path_default: false,
      }),
    ];
    mocks.getToolInstallationReports.mockImplementation(
      async (tools: string[]) =>
        tools.map((tool) =>
          report(tool, { installs: tool === "opencode" ? installs : [] }),
        ),
    );

    const metadata =
      deferred<
        Awaited<ReturnType<typeof settingsApi.getToolInstallationUpdates>>
      >();
    mocks.getToolInstallationUpdates.mockReturnValue(metadata.promise);
    await renderApps();
    const opencode = card("OpenCode");
    const versions = await opencode.findByRole("list", {
      name: "appsPage.columnVersion",
    });
    expect(versions).toHaveTextContent(/mise\s*2\.3\.2/);
    expect(versions).toHaveTextContent(/npm\s*1\.19\.6/);
    expect(opencode.getAllByText("mise")).toHaveLength(1);
    fireEvent.click(
      opencode.getByRole("button", { name: "appsPage.manageInstallations" }),
    );
    const details = within(
      await opencode.findByRole("region", {
        name: "appsPage.inspectInstallations",
      }),
    );
    expect(
      opencode.getByRole("button", { name: "appsPage.manageInstallations" }),
    ).toHaveAttribute("aria-busy", "true");
    expect(details.queryByText("common.loading")).toBeNull();
    await act(async () => metadata.resolve([]));
    expect(
      within(details.getByText(installs[0].path).closest("li")!).getByText(
        "mise",
      ),
    ).toBeInTheDocument();
    expect(
      within(details.getByText(installs[1].path).closest("li")!).getByText(
        "npm",
      ),
    ).toBeInTheDocument();
  });

  it("never combines a Windows installation source with the WSL version", async () => {
    const getVersions = mocks.getToolVersions.getMockImplementation()!;
    mocks.getToolVersions.mockImplementation(async (tools: string[]) =>
      ((await getVersions(tools)) as ToolVersions).map((tool) =>
        tool.name === "opencode"
          ? { ...tool, env_type: "wsl", wsl_distro: "Ubuntu", version: "2.3.2" }
          : tool,
      ),
    );
    mocks.getToolInstallationReports.mockImplementation(
      async (tools: string[]) =>
        tools.map((tool) =>
          report(
            tool,
            tool === "opencode"
              ? {
                  unmanaged: true,
                  installs: [
                    candidate(null, {
                      path: "C:/bin/opencode.exe",
                      source: "npm",
                      version: "1.19.6",
                    }),
                  ],
                }
              : {},
          ),
        ),
    );
    mocks.getPreferredToolInstallation.mockResolvedValue({
      supported: true,
      path: "C:/bin/opencode.exe",
      selected_path: "C:/bin/opencode.exe",
      selected_real_path: null,
      disabled_paths: [],
      selection: "explicit",
    });

    await renderApps();
    const opencode = card("OpenCode");
    expect(opencode.getByText("2.3.2")).toBeInTheDocument();
    expect(
      opencode.getByText("settings.envBadge.wsl · Ubuntu"),
    ).toBeInTheDocument();
    expect(opencode.queryByText("npm")).toBeNull();
    expect(
      opencode.queryByRole("combobox", { name: "appsPage.cliVersionChoice" }),
    ).toBeNull();
    expect(opencode.queryByText("1.19.6")).toBeNull();
    expect(
      opencode.queryByRole("button", { name: "appsPage.manageInstallations" }),
    ).toBeNull();
  });

  it("covers the installation channel catalog for all apps across macOS, Linux and Windows", async () => {
    const { TOOL_NAMES } = await import("@/components/apps/useToolManagement");
    const { getInstallChannels, OFFICIAL_INSTALL_PAGES } = await import(
      "@/components/apps/installCommands"
    );
    const { InstallCommandsDialog } = await import(
      "@/components/apps/InstallCommandsDialog"
    );
    const userAgent = vi.spyOn(navigator, "userAgent", "get");
    const hostPlatform = vi.spyOn(navigator, "platform", "get");
    const browserPlatforms = {
      macos: ["Macintosh; Intel Mac OS X", "MacIntel"],
      linux: ["X11; Linux x86_64", "Linux x86_64"],
      windows: ["Windows NT 10.0", "Win32"],
    } as const;
    const expected = {
      claude: [3, 3, 4],
      codex: [3, 2, 2],
      gemini: [4, 3, 2],
      grok: [2, 2, 2],
      opencode: [12, 14, 4],
      openclaw: [8, 10, 5],
      hermes: [3, 3, 3],
      pi: [2, 2, 2],
      mcode: [2, 2, 2],
      kimi: [2, 2, 2],
      dsh: [3, 3, 3],
      agy: [1, 1, 1],
      omp: [5, 5, 2],
      codebuddy: [6, 6, 5],
      copilot: [4, 4, 3],
      workbuddy: [0, 0, 0],
      "claude-desktop": [1, 0, 0],
    };
    try {
      for (const app of [
        ...TOOL_NAMES,
        "workbuddy",
        "claude-desktop",
      ] as const) {
        expect(OFFICIAL_INSTALL_PAGES[app]).toBeTruthy();
        for (const [index, platform] of (
          ["macos", "linux", "windows"] as const
        ).entries()) {
          userAgent.mockReturnValue(browserPlatforms[platform][0]);
          hostPlatform.mockReturnValue(browserPlatforms[platform][1]);
          const channels = getInstallChannels(app, platform);
          expect(channels, `${app} on ${platform}`).toHaveLength(
            expected[app][index],
          );
          expect(new Set(channels.map((channel) => channel.name)).size).toBe(
            channels.length,
          );
          for (const channel of channels) {
            if (channel.command === undefined) {
              expect(new URL(channel.guide).protocol).toBe("https:");
              expect(channel.method).toBeUndefined();
            } else {
              expect(channel.command.trim()).not.toBe("");
            }
            if (/^npm\b/.test(channel.name)) {
              expect(channel.method).toMatch(/^npm/);
            }
          }
          if (app === "codebuddy") {
            expect(channels.flatMap((channel) => channel.method ?? [])).toEqual(
              platform === "windows"
                ? ["npm", "pnpm", "yarn", "bun"]
                : ["curl", "homebrew", "npm", "pnpm", "yarn", "bun"],
            );
          }
          if (app === "hermes" && platform === "linux") {
            expect(channels).toContainEqual({
              name: "Nix",
              guide:
                "https://hermes-agent.nousresearch.com/docs/getting-started/nix-setup",
            });
          }
          if (platform === "windows")
            expect(
              channels.every(
                (channel) =>
                  channel.method !== "curl" && channel.method !== "homebrew",
              ),
            ).toBe(true);
          const onInstall = vi.fn().mockResolvedValue(undefined);
          const view = render(
            <QueryClientProvider client={createTestQueryClient()}>
              <InstallCommandsDialog
                initialTool={app}
                onClose={() => {}}
                onInstall={onInstall}
                isInstallBusy={() => false}
                isInstallQueued={() => false}
                toolVersionByName={
                  new Map([
                    [
                      app,
                      {
                        name: app,
                        version: null,
                        latest_version: null,
                        error: null,
                        installed_but_broken: false,
                        wsl_distro: null,
                        update_blocked_by_node: null,
                        env_type: platform,
                      },
                    ],
                  ])
                }
              />
            </QueryClientProvider>,
          );
          try {
            const dialog = within(screen.getByRole("dialog"));
            expect(
              dialog.queryAllByRole("button", { name: /^common.copy / }),
              `${app} on ${platform}`,
            ).toHaveLength(
              channels.filter((channel) => channel.command).length,
            );
            for (const channel of channels) {
              if (!channel.guide) continue;
              const openGuide = dialog.getByRole("button", {
                name: `appsPage.installDownload ${channel.name}`,
              });
              fireEvent.click(openGuide);
              expect(mocks.openExternal).toHaveBeenLastCalledWith(
                channel.guide,
              );
            }
            expect(
              dialog.queryAllByRole("button", {
                name: /^settings.toolInstall /,
              }),
            ).toHaveLength(channels.filter((channel) => channel.method).length);
            for (const channel of channels) {
              if (!channel.method) continue;
              await act(async () => {
                fireEvent.click(
                  dialog.getByRole("button", {
                    name: `settings.toolInstall ${channel.name}`,
                  }),
                );
              });
              expect(onInstall).toHaveBeenLastCalledWith(app, channel.method);
            }
            if (typeof OFFICIAL_INSTALL_PAGES[app] === "string") {
              expect(
                dialog.getByRole("button", {
                  name: "appsPage.installOfficialSite",
                }),
              ).toBeVisible();
            } else {
              expect(
                dialog.getByRole("button", {
                  name: "appsPage.installDownloadChina",
                }),
              ).toBeVisible();
              expect(
                dialog.getByRole("button", {
                  name: "appsPage.installDownloadGlobal",
                }),
              ).toBeVisible();
            }
          } finally {
            view.unmount();
          }
        }
      }
    } finally {
      userAgent.mockRestore();
      hostPlatform.mockRestore();
    }
  }, 20_000);

  it("checks every app without repeating the automatic choice for single-channel installs", async () => {
    const { TOOL_NAMES, TOOL_APP_IDS } = await import(
      "@/components/apps/useToolManagement"
    );
    const platforms = Object.fromEntries(
      TOOL_NAMES.map((tool) => [
        tool,
        {
          path: `/qa/bin/${tool}`,
          source: "npm",
          version: "2.0.0",
          runnable: true,
          error: null,
          is_path_default: true,
        },
      ]),
    );
    mocks.getToolVersions.mockImplementation(async (tools: string[]) =>
      tools.map((tool) => ({
        name: tool,
        version: "2.0.0",
        env_type: "native",
      })),
    );
    mocks.getOfficialClaudeDesktopInstallation.mockResolvedValue({
      path: "/Applications/Claude.app",
      version: "3.0.0",
    });
    mocks.getToolInstallationReports.mockImplementation(
      async (tools: string[]) =>
        tools.map((tool) => report(tool, { installs: [platforms[tool]] })),
    );
    const disabledTools = new Set<string>();
    const policyFor = (tool: string) => {
      const disabled = disabledTools.has(tool);
      return {
        supported: true,
        path: null,
        disabled_paths: disabled ? [platforms[tool].path] : [],
        selected_path: disabled ? null : platforms[tool].path,
        selected_real_path: null,
        selection: disabled ? "disabled" : "automatic",
      };
    };
    mocks.getPreferredToolInstallation.mockImplementation(
      async (tool: string) => policyFor(tool),
    );
    mocks.setPreferredToolInstallation.mockImplementation(
      async (tool: string, _path: string, enabled: boolean) => {
        if (enabled) disabledTools.delete(tool);
        else disabledTools.add(tool);
        return policyFor(tool);
      },
    );
    const view = await renderApps();
    mocks.getToolVersions.mockClear();
    fireEvent.click(
      screen.getByRole("button", { name: "appsPage.checkUpdates" }),
    );
    await waitFor(() => {
      for (const tool of TOOL_NAMES)
        expect(mocks.getToolVersions).toHaveBeenCalledWith(
          [tool],
          expect.anything(),
        );
      expect(
        screen.getByRole("button", { name: "appsPage.checkUpdates" }),
      ).toBeEnabled();
    });
    expect(mocks.getToolInstallationUpdates).not.toHaveBeenCalled();

    for (const tool of TOOL_NAMES) {
      const app = within(
        view.container.querySelector(
          `[data-tool-row="${TOOL_APP_IDS[tool]}"]`,
        )!,
      );
      const version = within(
        app.getByRole("list", { name: "appsPage.columnVersion" }),
      );
      expect(version.getByText("2.0.0")).toBeInTheDocument();
      expect(version.queryByRole("switch")).toBeNull();
      expect(app.queryByText(platforms[tool].path)).toBeNull();
      fireEvent.click(
        app.getByRole("button", { name: "appsPage.manageInstallations" }),
      );
      const management = within(
        await app.findByRole("region", {
          name: "appsPage.inspectInstallations",
        }),
      );
      expect(management.getByText(platforms[tool].path)).toBeInTheDocument();
      expect(
        management.getByRole("button", { name: "settings.toolUninstall" }),
      ).toBeInTheDocument();
      const toggle = await management.findByRole("switch", {
        name: "appsPage.cliEnable · npm",
      });
      expect(toggle).toBeChecked();
      expect(app.queryByRole("combobox")).toBeNull();
      if (tool === "dsh") {
        fireEvent.click(toggle);
        expect(
          await app.findByText("appsPage.cliInstallationDisabled"),
        ).toBeInTheDocument();
        expect(toggle).not.toBeChecked();
        expect(version.getByText("2.0.0")).toBeInTheDocument();
        fireEvent.click(toggle);
        await waitFor(() => expect(toggle).toBeChecked());
        expect(app.queryByText("appsPage.cliInstallationDisabled")).toBeNull();
      }
      expect(app.queryByText("appsPage.cliUsingAutomatic")).toBeNull();

      await waitFor(() =>
        expect(mocks.getToolInstallationUpdates).toHaveBeenCalledWith(
          tool,
          undefined,
          false,
        ),
      );
    }
    for (const desktop of ["claude-desktop", "workbuddy"]) {
      const app = within(
        view.container.querySelector(`[data-tool-row="${desktop}"]`)!,
      );
      expect(
        app.queryByRole("switch", { name: /^appsPage\.cliEnable · / }),
      ).toBeNull();
    }
    expect(mocks.runToolLifecycleAction).not.toHaveBeenCalled();
  }, 20_000);

  it("waits for Codex loading even while the check-updates label is hidden in the DOM", async () => {
    const getVersions = mocks.getToolVersions.getMockImplementation()!;
    const codexReady = deferred<void>();
    mocks.getToolVersions.mockImplementation(async (tools: string[]) => {
      if (tools.includes("codex")) await codexReady.promise;
      return getVersions(tools);
    });
    // Finish the first dynamic import before starting a DOM wait. Under the full
    // suite, transforming AppsPage can take longer than findByText's timeout.
    const { AppsPage } = await import("@/components/apps/AppsPage");
    mountApps(AppsPage);
    expect(await screen.findByText("appsPage.checkUpdates")).toHaveAttribute(
      "aria-hidden",
      "true",
    );
    expect(
      screen.queryByRole("button", { name: "appsPage.checkUpdates" }),
    ).toBeNull();
    expect(
      screen.getByRole("button", { name: "appsPage.checking" }),
    ).toBeDisabled();
    await act(async () => codexReady.resolve());
    await waitFor(() => expect(updateButton("Codex")).toBeEnabled());
  });
});
