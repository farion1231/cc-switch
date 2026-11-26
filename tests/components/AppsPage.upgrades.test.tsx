import { vi, expect, it, beforeEach, describe } from "vitest";
import {
  act,
  fireEvent,
  renderHook,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import type { ToolInstallationReport } from "@/lib/api/settings";
import { emitTauriEvent } from "../msw/tauriMocks";
import {
  mocks,
  deferred,
  report,
  candidate,
  upgraded,
  outdated,
  missing,
  card,
  isReady,
  updateButton,
  mountApps,
  renderApps,
  updateAllButton,
} from "./appsPageTestHelpers";
import type { ToolVersions } from "./appsPageTestHelpers";
import userEvent from "@testing-library/user-event";

describe("AppsPage concurrent CLI upgrades", () => {
  it.each([
    { blocked: false, fail: false, mode: "ok" },
    { blocked: false, fail: false, mode: "coupled" },
    { blocked: true, fail: false, mode: "ok" },
    { blocked: false, fail: true, mode: "ok" },
    { blocked: false, fail: false, mode: "missing" },
    { blocked: false, fail: false, mode: "unrunnable" },
    { blocked: false, fail: false, mode: "fallback" },
    { blocked: false, fail: false, mode: "prerelease" },
  ])(
    "updates eligible default and secondary copies without bypassing Node requirements (blocked=$blocked, fail=$fail, mode=$mode)",
    async ({ blocked, fail, mode }) => {
      outdated.clear();
      outdated.add("kimi");
      const primary = candidate(null, {
        path: "/home/test/.local/bin/kimi",
        real_path: "/home/test/kimi/native",
        version: "1.0.0",
        source: "curl",
        is_path_default: true,
      });
      const secondary = candidate(null, {
        path: "/home/test/npm/bin/kimi",
        real_path:
          "/home/test/npm/lib/node_modules/@moonshot-ai/kimi-code/bin/kimi",
        version: "1.0.0",
        source: "npm",
        is_path_default: false,
      });
      const plan = {
        tool: "kimi",
        path: secondary.path,
        real_path: secondary.real_path!,
        source: "npm",
        version: secondary.version,
        command: "update secondary",
      };
      mocks.getToolInstallationReports.mockImplementation(
        async (tools: string[]) =>
          tools.map((tool) =>
            report(
              tool,
              tool === "kimi" ? { installs: [primary, secondary] } : {},
            ),
          ),
      );
      mocks.getToolInstallationUpdates.mockImplementation(
        async (tool: string) =>
          tool === "kimi"
            ? [
                { installation: secondary, latest_version: "2.0.0", plan },
                ...(blocked || mode === "prerelease"
                  ? [
                      {
                        installation: primary,
                        latest_version:
                          mode === "prerelease" ? "2.1.0-beta.1" : "2.0.0",
                        update_blocked_by_node: blocked
                          ? {
                              required: ">=24",
                              current: "22",
                            }
                          : undefined,
                      },
                    ]
                  : []),
              ]
            : [],
      );
      let defaultRan = false;
      let secondaryRan = false;
      mocks.probeToolInstallations.mockImplementation(async (tools: string[]) =>
        tools.map((tool) =>
          report(
            tool,
            tool === "kimi"
              ? {
                  installs:
                    mode === "missing"
                      ? []
                      : defaultRan && mode === "fallback"
                        ? [secondary]
                        : [
                            {
                              ...primary,
                              version: defaultRan ? "2.0.0" : "1.0.0",
                              runnable: !(defaultRan && mode === "unrunnable"),
                              error:
                                defaultRan && mode === "unrunnable"
                                  ? "default probe failed"
                                  : null,
                            },
                            {
                              ...secondary,
                              version:
                                secondaryRan ||
                                (defaultRan && mode === "coupled")
                                  ? "2.0.0"
                                  : "1.0.0",
                            },
                          ],
                  needs_confirmation: true,
                  confirmation_token:
                    mode === "missing" ? undefined : "default-snapshot",
                  anchored: mode !== "missing",
                  command: mode === "missing" ? "" : "update default",
                }
              : {},
          ),
        ),
      );
      mocks.runToolLifecycleAction.mockImplementation(
        async (...args: unknown[]) => {
          if (fail)
            throw new Error(args[6] ? "secondary failed" : "default failed");
          if (!args[6]) defaultRan = true;
          else secondaryRan = true;
          upgraded.add("kimi");
        },
      );
      await renderApps();
      await waitFor(() => expect(updateAllButton()).toBeEnabled());
      fireEvent.click(updateAllButton());
      const dialog = await screen.findByRole("dialog");
      fireEvent.click(
        within(dialog).getByRole("button", {
          name: "settings.toolUpgradeConfirmBtn",
        }),
      );
      await waitFor(() =>
        expect(mocks.runToolLifecycleAction).toHaveBeenCalledTimes(
          blocked ||
            mode === "prerelease" ||
            mode === "missing" ||
            mode === "coupled"
            ? 1
            : 2,
        ),
      );
      if (!blocked && mode !== "prerelease" && mode !== "missing")
        expect(mocks.runToolLifecycleAction.mock.calls[0]).toEqual([
          ["kimi"],
          "update",
          {},
          undefined,
          "default-snapshot",
        ]);
      if (mode === "coupled") {
        await waitFor(() => expect(mocks.success).toHaveBeenCalled());
        expect(mocks.warning).not.toHaveBeenCalled();
        expect(mocks.error).not.toHaveBeenCalled();
      } else {
        expect(
          mocks.runToolLifecycleAction.mock.calls[
            blocked || mode === "prerelease" || mode === "missing" ? 0 : 1
          ][6],
        ).toEqual(plan);
      }
      if (mode === "unrunnable" || mode === "fallback")
        await waitFor(() =>
          expect(mocks.warning).toHaveBeenCalledWith(
            expect.anything(),
            expect.objectContaining({
              description: expect.stringContaining(
                mode === "fallback"
                  ? "appsPage.installUpdateUnverified"
                  : "default probe failed",
              ),
            }),
          ),
        );
      if (fail)
        await waitFor(() =>
          expect(mocks.error).toHaveBeenCalledWith(
            expect.anything(),
            expect.objectContaining({
              description: expect.stringMatching(
                /default failed[\s\S]*secondary failed/,
              ),
            }),
          ),
        );
    },
    15000,
  );

  it.each([false, true])(
    "waits for current prerelease ownership before enabling the complete batch (refresh=%s)",
    async (refresh) => {
      outdated.clear();
      outdated.add("claude");
      outdated.add("kimi");
      const original = mocks.getToolVersions.getMockImplementation()!;
      mocks.getToolVersions.mockImplementation(async (tools: string[]) =>
        (await original(tools)).map((tool: ToolVersions[number]) =>
          tool.name === "claude"
            ? { ...tool, version: "2.1.0", latest_version: "2.2.0-next.1" }
            : tool,
        ),
      );
      const pending = deferred<ToolInstallationReport[]>();
      mocks.getToolInstallationReports.mockReturnValue(pending.promise);
      if (refresh)
        mocks.getToolInstallationReports.mockResolvedValueOnce([
          report("claude", {
            installs: [
              candidate(null, { source: "npm", is_path_default: true }),
            ],
          }),
          report("kimi"),
        ]);
      await renderApps();
      if (refresh) {
        await waitFor(() => expect(updateAllButton()).toBeEnabled());
        fireEvent.click(
          screen.getByRole("button", { name: "appsPage.checkUpdates" }),
        );
        await waitFor(() =>
          expect(mocks.getToolInstallationReports).toHaveBeenCalledTimes(2),
        );
      }
      await waitFor(() =>
        expect(screen.getByText("appsPage.checkUpdates")).toHaveAttribute(
          "aria-hidden",
          "false",
        ),
      );
      await waitFor(() => expect(updateAllButton()).toBeDisabled());
      await act(async () =>
        pending.resolve([
          report("claude", {
            installs: [
              candidate(null, {
                source: "curl",
                version: "2.1.0",
                is_path_default: true,
              }),
            ],
          }),
          report("kimi"),
        ]),
      );
      await waitFor(() => expect(updateAllButton()).toBeEnabled());
      mocks.probeToolInstallations.mockImplementation(async (tools: string[]) =>
        tools.map((tool) => report(tool, { needs_confirmation: true })),
      );
      fireEvent.click(updateAllButton());
      await waitFor(() =>
        expect(mocks.probeToolInstallations).toHaveBeenCalledWith([
          "claude",
          "kimi",
        ]),
      );
      expect(mocks.runToolLifecycleAction).not.toHaveBeenCalled();
    },
  );

  it("does not execute a rejected npm prerelease plan through the generic updater", async () => {
    outdated.clear();
    outdated.add("claude");
    const original = mocks.getToolVersions.getMockImplementation()!;
    mocks.getToolVersions.mockImplementation(async (tools: string[]) =>
      (await original(tools)).map((tool: ToolVersions[number]) =>
        tool.name === "claude"
          ? { ...tool, version: "2.1.0", latest_version: "2.2.0-next.1" }
          : tool,
      ),
    );
    const primary = candidate(null, {
      source: "npm",
      version: "2.1.0",
      is_path_default: true,
    });
    mocks.probeToolInstallations.mockResolvedValue([
      report("claude", {
        installs: [primary],
        anchored: true,
        confirmation_token: "npm-snapshot",
        update_plan: {
          tool: "claude",
          path: primary.path,
          real_path: primary.real_path!,
          version: "2.1.0",
          source: "npm",
          command: "npm install -g @anthropic-ai/claude-code@latest",
        },
      }),
    ]);
    const snapshot = mocks.probeToolInstallations.getMockImplementation()!;
    mocks.getToolInstallationReports.mockImplementation(
      async (tools: string[]) =>
        tools
          .map((tool) => (tool === "claude" ? undefined : report(tool)))
          .filter(Boolean)
          .concat(await snapshot(["claude"])),
    );
    await renderApps();
    await waitFor(() =>
      expect(
        screen.queryByRole("button", { name: /settings\.updateAllTools/ }),
      ).toBeNull(),
    );
    fireEvent.click(updateButton("Claude Code"));
    await waitFor(() =>
      expect(mocks.warning).toHaveBeenCalledWith(
        "appsPage.installPrereleaseUpdate",
      ),
    );
    expect(mocks.runToolLifecycleAction).not.toHaveBeenCalled();
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("lets different tools preflight and submit together while blocking duplicate clicks", async () => {
    const claudeProbe = deferred<ToolInstallationReport[]>();
    const codexProbe = deferred<ToolInstallationReport[]>();
    const claudeRun = deferred<void>();
    const codexRun = deferred<void>();
    mocks.probeToolInstallations.mockImplementation(([tool]: string[]) =>
      tool === "claude" ? claudeProbe.promise : codexProbe.promise,
    );
    mocks.runToolLifecycleAction.mockImplementation(
      async ([tool]: string[]) => {
        await (tool === "claude" ? claudeRun.promise : codexRun.promise);
        upgraded.add(tool);
      },
    );
    await renderApps();

    fireEvent.click(updateButton("Claude Code"));
    fireEvent.click(updateButton("Claude Code"));
    expect(updateButton("Claude Code")).toBeDisabled();
    expect(updateButton("Codex")).toBeEnabled();
    fireEvent.click(updateButton("Codex"));
    expect(mocks.probeToolInstallations.mock.calls).toEqual([
      [["claude"]],
      [["codex"]],
    ]);

    await act(async () => {
      claudeProbe.resolve([report("claude")]);
      codexProbe.resolve([report("codex")]);
    });
    expect(mocks.runToolLifecycleAction).toHaveBeenCalledTimes(2);
    expect(updateButton("Claude Code")).toBeDisabled();
    expect(updateButton("Codex")).toBeDisabled();
    expect(updateButton("Gemini CLI")).toBeEnabled();

    await act(async () => codexRun.resolve());
    expect(isReady("Codex")).toBe(true);
    expect(updateButton("Claude Code")).toBeDisabled();
    await act(async () => claudeRun.resolve());
    expect(isReady("Claude Code")).toBe(true);
    expect(mocks.success).toHaveBeenCalledTimes(2);
  });

  it("submits the remaining tools while a single upgrade is already running", async () => {
    const runs = new Map(
      ["claude", "codex", "gemini"].map((name) => [name, deferred<void>()]),
    );
    mocks.runToolLifecycleAction.mockImplementation(
      async ([tool]: string[]) => {
        await runs.get(tool)!.promise;
        upgraded.add(tool);
      },
    );
    await renderApps();
    fireEvent.click(updateButton("Claude Code"));
    await waitFor(() =>
      expect(mocks.runToolLifecycleAction).toHaveBeenCalledTimes(1),
    );
    const updateAll = updateAllButton();
    expect(updateAll).toBeEnabled();
    fireEvent.click(updateAll);
    await waitFor(() =>
      expect(mocks.runToolLifecycleAction).toHaveBeenCalledTimes(3),
    );
    expect(mocks.probeToolInstallations).toHaveBeenNthCalledWith(2, [
      "codex",
      "gemini",
    ]);
    expect(
      mocks.runToolLifecycleAction.mock.calls.map(([tools]) => tools),
    ).toEqual([["claude"], ["codex"], ["gemini"]]);

    await act(async () => {
      runs.get("gemini")!.resolve();
      runs.get("codex")!.resolve();
    });
    expect(isReady("Gemini CLI")).toBe(true);
    expect(isReady("Codex")).toBe(true);
    expect(updateButton("Claude Code")).toBeDisabled();
    await act(async () => runs.get("claude")!.resolve());
  });

  it("restores an ongoing upgrade after remount and receives its completion", async () => {
    const running = deferred<void>();
    mocks.runToolLifecycleAction.mockImplementationOnce(async () => {
      await running.promise;
      upgraded.add("claude");
      missing.delete("claude");
    });
    const actionButton = () => updateButton("Claude Code");
    const view = await renderApps();
    fireEvent.click(actionButton());
    await waitFor(() =>
      expect(mocks.runToolLifecycleAction).toHaveBeenCalledTimes(1),
    );
    view.unmount();
    await renderApps();
    const versionChecks = mocks.getToolVersions.mock.calls.length;
    expect(actionButton()).toBeDisabled();
    expect(actionButton()).toHaveAttribute("aria-busy", "true");
    expect(updateButton("Codex")).toBeEnabled();
    fireEvent.click(actionButton());
    expect(mocks.runToolLifecycleAction).toHaveBeenCalledTimes(1);
    expect(mocks.getToolVersions).toHaveBeenCalledTimes(versionChecks);
    expect(mocks.info).not.toHaveBeenCalled();
    expect(mocks.error).not.toHaveBeenCalled();
    expect(mocks.warning).not.toHaveBeenCalled();
    expect(mocks.success).not.toHaveBeenCalled();

    await act(async () => running.resolve());
    expect(isReady("Claude Code")).toBe(true);
    expect(
      screen.getByRole("button", { name: "appsPage.checkUpdates" }),
    ).toBeEnabled();
    expect(mocks.success).toHaveBeenCalledTimes(1);
  });

  it("shows an informational toast when the backend reports an ongoing upgrade", async () => {
    mocks.runToolLifecycleAction.mockRejectedValueOnce(
      "TOOL_ACTION_IN_PROGRESS",
    );
    await renderApps();
    const versionChecks = mocks.getToolVersions.mock.calls.length;
    fireEvent.click(updateButton("Claude Code"));
    await waitFor(() =>
      expect(mocks.info).toHaveBeenCalledWith("settings.toolActionInProgress", {
        description: "settings.toolActionInProgressDetail",
        closeButton: true,
      }),
    );
    expect(mocks.getToolVersions).toHaveBeenCalledTimes(versionChecks);
    expect(mocks.error).not.toHaveBeenCalled();
    expect(mocks.warning).not.toHaveBeenCalled();
    expect(mocks.success).not.toHaveBeenCalled();
  });

  it("ignores a late probe from the old page after the remounted page upgrades a tool", async () => {
    const stale = deferred<ToolVersions>();
    const getVersions = mocks.getToolVersions.getMockImplementation()!;
    let firstClaudeProbe = true;
    let oldResult: ToolVersions = [];
    mocks.getToolVersions.mockImplementation(async (tools: string[]) => {
      if (tools.includes("claude") && firstClaudeProbe) {
        firstClaudeProbe = false;
        oldResult = await getVersions(tools);
        return stale.promise;
      }
      return getVersions(tools);
    });
    const { AppsPage } = await import("@/components/apps/AppsPage");
    const { TOOL_NAMES } = await import("@/components/apps/useToolManagement");
    const view = mountApps(AppsPage);
    await waitFor(() =>
      expect(mocks.getToolVersions).toHaveBeenCalledTimes(TOOL_NAMES.length),
    );
    view.unmount();
    const remounted = await renderApps();
    fireEvent.click(updateButton("Claude Code"));
    await waitFor(() => expect(isReady("Claude Code")).toBe(true));

    await act(async () => stale.resolve(oldResult));
    expect(isReady("Claude Code")).toBe(true);
    expect(card("Claude Code").queryByText("1.0.0")).not.toBeInTheDocument();
    remounted.unmount();
    await renderApps();
    expect(isReady("Claude Code")).toBe(true);
  });

  it("preserves WSL execution and refresh parameters when confirming a mixed batch after remount", async () => {
    const preflight = deferred<ToolInstallationReport[]>();
    const getVersions = mocks.getToolVersions.getMockImplementation()!;
    mocks.getToolVersions.mockImplementation(async (tools: string[]) =>
      ((await getVersions(tools)) as ToolVersions).map((tool) =>
        tool.name === "claude"
          ? { ...tool, env_type: "wsl", wsl_distro: "Ubuntu" }
          : tool,
      ),
    );
    mocks.probeToolInstallations.mockImplementationOnce(
      () => preflight.promise,
    );
    const scrollIntoView = Object.getOwnPropertyDescriptor(
      HTMLElement.prototype,
      "scrollIntoView",
    );
    Object.defineProperty(HTMLElement.prototype, "scrollIntoView", {
      configurable: true,
      value: vi.fn(),
    });
    try {
      const user = userEvent.setup();
      const view = await renderApps();
      await user.click(card("Claude Code").getAllByRole("combobox")[0]);
      await user.click(screen.getByRole("option", { name: "bash" }));
      await waitFor(() => expect(updateButton("Claude Code")).toBeEnabled());
      await user.click(card("Claude Code").getAllByRole("combobox")[1]);
      await user.click(screen.getByRole("option", { name: "-lic" }));
      await waitFor(() => expect(updateButton("Claude Code")).toBeEnabled());
      fireEvent.click(updateAllButton());
      view.unmount();
      // WSL tools do not request confirmation themselves; a native tool in
      // the same batch can hold the whole batch pending confirmation.
      await act(async () =>
        preflight.resolve([
          report("claude"),
          report("codex", { needs_confirmation: true }),
          report("gemini"),
        ]),
      );
      await renderApps();
      mocks.getToolVersions.mockClear();
      fireEvent.click(
        screen.getByRole("button", { name: "settings.toolUpgradeConfirmBtn" }),
      );
      await waitFor(() => expect(mocks.success).toHaveBeenCalledTimes(1));
      const preferences = {
        claude: { wslShell: "bash", wslShellFlag: "-lic" },
      };
      expect(mocks.runToolLifecycleAction).toHaveBeenCalledWith(
        ["claude"],
        "update",
        preferences,
      );
      expect(mocks.getToolVersions).toHaveBeenCalledWith(
        ["claude"],
        preferences,
      );
    } finally {
      if (scrollIntoView) {
        Object.defineProperty(
          HTMLElement.prototype,
          "scrollIntoView",
          scrollIntoView,
        );
      } else {
        Reflect.deleteProperty(HTMLElement.prototype, "scrollIntoView");
      }
    }
  });

  it("preserves batch progress across remounts and updates each completed tool", async () => {
    const runs = new Map(
      ["claude", "codex", "gemini"].map((name) => [name, deferred<void>()]),
    );
    mocks.runToolLifecycleAction.mockImplementation(
      async ([tool]: string[]) => {
        await runs.get(tool)!.promise;
        upgraded.add(tool);
      },
    );
    const view = await renderApps();
    fireEvent.click(updateAllButton());
    await waitFor(() =>
      expect(mocks.runToolLifecycleAction).toHaveBeenCalledTimes(3),
    );
    view.unmount();
    await act(async () => runs.get("codex")!.resolve());
    await renderApps();
    const updateAll = updateAllButton();
    expect(updateAll).toBeDisabled();
    expect(updateAll).toHaveAttribute("aria-busy", "true");
    expect(isReady("Codex")).toBe(true);
    expect(updateButton("Claude Code")).toHaveAttribute("aria-busy", "true");
    expect(updateButton("Gemini CLI")).toBeDisabled();

    await act(async () => runs.get("gemini")!.resolve());
    expect(isReady("Gemini CLI")).toBe(true);
    expect(updateButton("Claude Code")).toBeDisabled();
    expect(updateAll).toHaveAttribute("aria-busy", "true");
    await act(async () => runs.get("claude")!.resolve());
    expect(isReady("Claude Code")).toBe(true);
    // 全部升级完、没有可升级的了：按钮收起
    expect(
      screen.queryByRole("button", { name: /settings\.updateAllTools/ }),
    ).toBeNull();
    expect(
      screen.getByRole("button", { name: "appsPage.checkUpdates" }),
    ).toBeEnabled();
    expect(mocks.runToolLifecycleAction).toHaveBeenCalledTimes(3);
    expect(mocks.success).toHaveBeenCalledTimes(1);
  });

  it("preserves preflight and its confirmation when navigating away and back", async () => {
    const preflight = deferred<ToolInstallationReport[]>();
    mocks.probeToolInstallations.mockImplementationOnce(
      () => preflight.promise,
    );
    const view = await renderApps();
    fireEvent.click(updateButton("Claude Code"));
    view.unmount();
    const remounted = await renderApps();
    expect(updateButton("Claude Code")).toBeDisabled();
    expect(updateButton("Claude Code")).toHaveAttribute("aria-busy", "true");
    expect(mocks.probeToolInstallations).toHaveBeenCalledTimes(1);
    await act(async () =>
      preflight.resolve([report("claude", { needs_confirmation: true })]),
    );
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    remounted.unmount();
    await renderApps();
    expect(
      within(screen.getByRole("dialog")).getByText("Claude Code"),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "common.cancel" }));
    expect(updateButton("Claude Code")).toBeEnabled();
    expect(mocks.runToolLifecycleAction).not.toHaveBeenCalled();
    fireEvent.click(updateButton("Claude Code"));
    await waitFor(() => expect(isReady("Claude Code")).toBe(true));
  });

  it("unlocks a failed background task after remount so it can be retried", async () => {
    const running = deferred<void>();
    mocks.runToolLifecycleAction.mockImplementationOnce(() => running.promise);
    const errorLog = vi.spyOn(console, "error").mockImplementation(() => {});
    try {
      const view = await renderApps();
      fireEvent.click(updateButton("Claude Code"));
      await waitFor(() =>
        expect(mocks.runToolLifecycleAction).toHaveBeenCalledTimes(1),
      );
      view.unmount();
      await renderApps();
      expect(updateButton("Claude Code")).toBeDisabled();
      await act(async () => running.reject(new Error("installer failed")));
      expect(updateButton("Claude Code")).toBeEnabled();
      expect(mocks.error).toHaveBeenCalledWith("settings.toolActionFailed", {
        description: "installer failed",
        closeButton: true,
      });
      fireEvent.click(updateButton("Claude Code"));
      await waitFor(() => expect(isReady("Claude Code")).toBe(true));
    } finally {
      errorLog.mockRestore();
    }
  });

  it("does not report success or failure when all requested tools are already running", async () => {
    mocks.runToolLifecycleAction.mockRejectedValue("TOOL_ACTION_IN_PROGRESS");
    await renderApps();
    const versionChecks = mocks.getToolVersions.mock.calls.length;
    fireEvent.click(updateAllButton());
    await waitFor(() => expect(mocks.info).toHaveBeenCalledTimes(3));
    expect(mocks.getToolVersions).toHaveBeenCalledTimes(versionChecks);
    expect(mocks.success).not.toHaveBeenCalled();
    expect(mocks.warning).not.toHaveBeenCalled();
    expect(mocks.error).not.toHaveBeenCalled();
  });

  it("completes other tools without counting an ongoing task as a failure", async () => {
    mocks.runToolLifecycleAction.mockImplementation(
      async ([tool]: string[]) => {
        if (tool === "claude") throw "TOOL_ACTION_IN_PROGRESS";
        upgraded.add(tool);
      },
    );
    await renderApps();
    fireEvent.click(updateAllButton());
    await waitFor(() => expect(mocks.success).toHaveBeenCalledTimes(1));
    expect(mocks.info).toHaveBeenCalledTimes(1);
    expect(isReady("Codex")).toBe(true);
    expect(isReady("Gemini CLI")).toBe(true);
    expect(mocks.warning).not.toHaveBeenCalled();
    expect(mocks.error).not.toHaveBeenCalled();
  });

  it("keeps genuine failures in the batch summary while excluding ongoing tasks", async () => {
    mocks.runToolLifecycleAction.mockImplementation(
      async ([tool]: string[]) => {
        if (tool === "claude") throw "TOOL_ACTION_IN_PROGRESS";
        if (tool === "codex") throw new Error("installer failed");
        upgraded.add(tool);
      },
    );
    const errorLog = vi.spyOn(console, "error").mockImplementation(() => {});
    try {
      await renderApps();
      fireEvent.click(updateAllButton());
      await waitFor(() =>
        expect(mocks.warning).toHaveBeenCalledWith(
          "settings.toolActionPartial",
          { description: "Codex: installer failed", closeButton: true },
        ),
      );
      expect(mocks.info).toHaveBeenCalledTimes(1);
      expect(mocks.success).not.toHaveBeenCalled();
      expect(mocks.error).not.toHaveBeenCalled();
      expect(errorLog).toHaveBeenCalledTimes(1);
    } finally {
      errorLog.mockRestore();
    }
  });

  it("releases a failed tool for retry without unlocking another running tool", async () => {
    const firstRun = deferred<void>();
    const retryRun = deferred<void>();
    const codexRun = deferred<void>();
    const geminiRun = deferred<void>();
    let claudeAttempts = 0;
    mocks.runToolLifecycleAction.mockImplementation(
      async ([tool]: string[]) => {
        const run =
          tool === "claude"
            ? ++claudeAttempts === 1
              ? firstRun
              : retryRun
            : tool === "codex"
              ? codexRun
              : geminiRun;
        await run.promise;
        upgraded.add(tool);
      },
    );
    const errorLog = vi.spyOn(console, "error").mockImplementation(() => {});
    await renderApps();
    fireEvent.click(updateAllButton());
    await waitFor(() =>
      expect(mocks.runToolLifecycleAction).toHaveBeenCalledTimes(3),
    );
    await act(async () => firstRun.reject(new Error("upgrade failed")));
    expect(updateButton("Claude Code")).toBeEnabled();
    expect(updateButton("Codex")).toBeDisabled();
    fireEvent.click(updateButton("Claude Code"));
    await waitFor(() =>
      expect(mocks.runToolLifecycleAction).toHaveBeenCalledTimes(4),
    );
    await act(async () => {
      codexRun.resolve();
      geminiRun.resolve();
    });
    expect(updateButton("Claude Code")).toBeDisabled();
    expect(mocks.warning).toHaveBeenCalledWith(
      "settings.toolActionPartial",
      expect.objectContaining({ description: "Claude Code: upgrade failed" }),
    );
    await act(async () => retryRun.resolve());
    expect(isReady("Claude Code")).toBe(true);
    errorLog.mockRestore();
  });

  it("queues concurrent confirmation requests and releases only the cancelled tool", async () => {
    const claudeProbe = deferred<ToolInstallationReport[]>();
    const codexProbe = deferred<ToolInstallationReport[]>();
    const claudeRun = deferred<void>();
    mocks.probeToolInstallations.mockImplementation(([tool]: string[]) =>
      tool === "claude" ? claudeProbe.promise : codexProbe.promise,
    );
    mocks.runToolLifecycleAction.mockImplementation(
      async ([tool]: string[]) => {
        await claudeRun.promise;
        upgraded.add(tool);
      },
    );
    await renderApps();
    fireEvent.click(updateButton("Claude Code"));
    fireEvent.click(updateButton("Codex"));
    await act(async () => {
      claudeProbe.resolve([report("claude", { needs_confirmation: true })]);
      codexProbe.resolve([report("codex", { needs_confirmation: true })]);
    });
    expect(
      within(screen.getByRole("dialog")).getByText("Claude Code"),
    ).toBeInTheDocument();
    expect(
      within(screen.getByRole("dialog")).queryByText("Codex"),
    ).not.toBeInTheDocument();
    expect(mocks.runToolLifecycleAction).not.toHaveBeenCalled();
    fireEvent.click(
      screen.getByRole("button", { name: "settings.toolUpgradeConfirmBtn" }),
    );
    expect(
      within(screen.getByRole("dialog")).getByText("Codex"),
    ).toBeInTheDocument();
    expect(mocks.runToolLifecycleAction).toHaveBeenCalledTimes(1);
    expect(mocks.runToolLifecycleAction).toHaveBeenCalledWith(
      ["claude"],
      "update",
      {},
    );
    fireEvent.click(screen.getByRole("button", { name: "common.cancel" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(updateButton("Codex")).toBeEnabled();
    expect(updateButton("Claude Code")).toBeDisabled();
    await act(async () => claudeRun.resolve());
  });

  it.each(["double click", "held Enter", "held Space"])(
    "does not confirm the next queued plan with a %s",
    async (gesture) => {
      const claudeProbe = deferred<ToolInstallationReport[]>();
      const codexProbe = deferred<ToolInstallationReport[]>();
      const run = deferred<void>();
      mocks.probeToolInstallations.mockImplementation(([tool]: string[]) =>
        tool === "claude" ? claudeProbe.promise : codexProbe.promise,
      );
      mocks.runToolLifecycleAction.mockImplementation(() => run.promise);
      const user = userEvent.setup();
      await renderApps();
      fireEvent.click(updateButton("Claude Code"));
      fireEvent.click(updateButton("Codex"));
      await act(async () => {
        claudeProbe.resolve([report("claude", { needs_confirmation: true })]);
        codexProbe.resolve([report("codex", { needs_confirmation: true })]);
      });
      const confirm = screen.getByRole("button", {
        name: "settings.toolUpgradeConfirmBtn",
      });
      if (gesture === "double click") {
        await user.dblClick(confirm);
      } else {
        confirm.focus();
        await user.keyboard(
          gesture === "held Enter" ? "{Enter>3/}" : "[Space>3/]",
        );
      }
      expect(mocks.runToolLifecycleAction.mock.calls).toEqual([
        [["claude"], "update", {}],
      ]);
      expect(
        within(screen.getByRole("dialog")).getByText("Codex"),
      ).toBeInTheDocument();
      if (gesture !== "double click") {
        expect(
          screen.getByRole("heading", {
            name: "settings.toolUpgradeConfirmTitle",
          }),
        ).toHaveFocus();
      }
      // 第二项仍可通过一次新的、明确的操作正常确认。
      await user.click(confirm);
      expect(mocks.runToolLifecycleAction.mock.calls).toEqual([
        [["claude"], "update", {}],
        [["codex"], "update", {}],
      ]);
      await act(async () => run.resolve());
    },
  );

  it("does not cancel the next queued plan with a double click", async () => {
    const claudeProbe = deferred<ToolInstallationReport[]>();
    const codexProbe = deferred<ToolInstallationReport[]>();
    mocks.probeToolInstallations.mockImplementation(([tool]: string[]) =>
      tool === "claude" ? claudeProbe.promise : codexProbe.promise,
    );
    const user = userEvent.setup();
    await renderApps();
    fireEvent.click(updateButton("Claude Code"));
    fireEvent.click(updateButton("Codex"));
    await act(async () => {
      claudeProbe.resolve([report("claude", { needs_confirmation: true })]);
      codexProbe.resolve([report("codex", { needs_confirmation: true })]);
    });
    await user.dblClick(screen.getByRole("button", { name: "common.cancel" }));
    expect(
      within(screen.getByRole("dialog")).getByText("Codex"),
    ).toBeInTheDocument();
    expect(mocks.runToolLifecycleAction).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "common.cancel" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(updateButton("Claude Code")).toBeEnabled();
    expect(updateButton("Codex")).toBeEnabled();
  });

  it("passes the reviewed upgrade token when confirming an installation", async () => {
    const user = userEvent.setup();
    mocks.probeToolInstallations.mockResolvedValueOnce([
      report("claude", {
        needs_confirmation: true,
        confirmation_token: "reviewed-token",
      }),
    ]);
    await renderApps();
    await user.click(updateButton("Claude Code"));
    const dialog = within(await screen.findByRole("dialog"));
    await user.click(
      dialog.getByRole("button", { name: "settings.toolUpgradeConfirmBtn" }),
    );
    expect(mocks.runToolLifecycleAction).toHaveBeenCalledWith(
      ["claude"],
      "update",
      {},
      undefined,
      "reviewed-token",
    );
  });

  it("opens the selected app’s install commands while another app is upgrading", async () => {
    const claudeRun = deferred<void>();
    missing.add("codex");
    mocks.runToolLifecycleAction.mockImplementation(
      async ([tool]: string[]) => {
        if (tool === "claude") await claudeRun.promise;
        upgraded.add(tool);
        missing.delete(tool);
      },
    );
    await renderApps();
    fireEvent.click(updateButton("Claude Code"));
    await waitFor(() =>
      expect(mocks.runToolLifecycleAction).toHaveBeenCalledTimes(1),
    );
    const install = card("Codex").getByRole("button", {
      name: "settings.toolInstall",
    });
    expect(install).toBeEnabled();
    fireEvent.click(install);
    const dialog = within(screen.getByRole("dialog"));
    expect(dialog.getByRole("heading", { name: "Codex" })).toBeVisible();
    expect(
      dialog.getByText("npm install -g @openai/codex@latest"),
    ).toBeVisible();
    expect(mocks.runToolLifecycleAction).toHaveBeenCalledTimes(1);
    fireEvent.click(
      dialog.getByRole("button", { name: "appsPage.installOfficialSite" }),
    );
    expect(mocks.openExternal).toHaveBeenLastCalledWith(
      "https://github.com/openai/codex/releases/latest",
    );
    fireEvent.click(dialog.getByRole("button", { name: "common.close" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(install).toBeEnabled();
    expect(updateButton("Claude Code")).toBeDisabled();
    await act(async () => claudeRun.resolve());
  });

  it.each([
    ["curl", "curl", false],
    ["Homebrew", "homebrew", false],
    ["curl", "curl", true],
  ] as const)(
    "shows Codex installation actions for %s (%s, desktop=%s)",
    async (label, method, withDesktop) => {
      const platform = vi
        .spyOn(navigator, "userAgent", "get")
        .mockReturnValue("Macintosh; Intel Mac OS X");
      const run = deferred<void>();
      try {
        missing.add("codex");
        const getVersions = mocks.getToolVersions.getMockImplementation()!;
        mocks.getToolVersions.mockImplementation(async (tools: string[]) =>
          (await getVersions(tools)).map((tool: ToolVersions[number]) =>
            tool.name === "codex" ? { ...tool, env_type: "macos" } : tool,
          ),
        );
        if (withDesktop) {
          const getVersions = mocks.getToolVersions.getMockImplementation()!;
          mocks.getToolVersions.mockImplementation(async (tools: string[]) =>
            (await getVersions(tools)).map((tool: ToolVersions[number]) =>
              tool.name === "codex"
                ? {
                    ...tool,
                    version: null,
                    desktop_managed: true,
                    env_type: "macos",
                    desktop_installation: {
                      path: "/Applications/ChatGPT.app",
                      version: "26.1117.41037",
                    },
                  }
                : tool,
            ),
          );
        }
        mocks.runToolLifecycleAction.mockImplementation(async () => {
          await run.promise;
          missing.delete("codex");
        });
        const newPath = "/tmp/cc-switch-qa/bin/codex";
        const newCopy = candidate(null, {
          path: newPath,
          source: method,
          version: "2.0.0",
        });
        mocks.probeToolInstallations.mockImplementation(async () => [
          report("codex", {
            installs: missing.has("codex")
              ? []
              : [
                  newCopy,
                  candidate(null, {
                    path: "/tmp/cc-switch-qa/npm/bin/codex",
                    source: "npm",
                    version: "1.0.0",
                    is_path_default: false,
                  }),
                ],
            is_conflict: true,
          }),
        ]);
        await renderApps();
        if (withDesktop) {
          const user = userEvent.setup();
          await user.click(
            screen.getByRole("button", { name: "appsPage.moreActions" }),
          );
          await user.click(
            await screen.findByRole("menuitem", {
              name: "appsPage.manualCommands",
            }),
          );
          await user.click(
            within(screen.getByRole("dialog")).getByRole("button", {
              name: "Codex",
            }),
          );
        } else {
          fireEvent.click(
            card("Codex").getByRole("button", { name: "settings.toolInstall" }),
          );
        }
        const dialog = within(screen.getByRole("dialog"));
        if (withDesktop) {
          const status = dialog.getAllByText(
            "appsPage.installAlreadyInstalled",
          )[0];
          expect(
            within(status.parentElement!).getByText(
              "appsPage.installedFromOfficialClient",
            ),
          ).toBeVisible();
        }
        const install = dialog.getByRole("button", {
          name: `settings.toolInstall ${label}`,
        });
        expect(
          dialog.getByRole("button", { name: `common.copy ${label}` })
            .nextElementSibling,
        ).toBe(install);
        fireEvent.click(install);
        await waitFor(() =>
          expect(mocks.runToolLifecycleAction).toHaveBeenCalledWith(
            ["codex"],
            "install",
            {},
            undefined,
            undefined,
            method,
          ),
        );
        expect(install).toBeDisabled();
        expect(
          dialog
            .queryByRole("button", { name: "settings.toolInstall npm" })
            ?.hasAttribute("disabled") ?? true,
        ).toBe(true);
        await act(async () => run.resolve());
        await waitFor(() =>
          expect(
            dialog.queryByRole("button", {
              name: `settings.toolInstall ${label}`,
            }),
          ).toBeNull(),
        );
        const status = dialog.getAllByText(
          "appsPage.installAlreadyInstalled",
        )[0];
        const channels = within(status.parentElement!);
        expect(
          channels.getByText(method === "homebrew" ? "Homebrew" : "curl"),
        ).toBeVisible();
        if (withDesktop) {
          expect(
            channels.getByText("appsPage.installedFromOfficialClient"),
          ).toBeVisible();
        }
        expect(
          dialog.getByRole("button", { name: `common.copy ${label}` }),
        ).toBeEnabled();
        expect(mocks.success).toHaveBeenCalledWith(
          "settings.toolActionDone",
          expect.anything(),
        );
        fireEvent.click(dialog.getByRole("button", { name: "common.close" }));
        if (withDesktop && !card("Codex").queryByText(newPath)) {
          fireEvent.click(
            card("Codex").getByRole("button", {
              name: "appsPage.manageInstallations",
            }),
          );
        }
        expect(await card("Codex").findByText(newPath)).toBeInTheDocument();
        expect(
          card("Codex").getByRole("button", {
            name: "appsPage.manageInstallations",
          }),
        ).toBeInTheDocument();
        expect(
          card("Codex").queryByRole("button", { name: "settings.toolInstall" }),
        ).toBeNull();
      } finally {
        platform.mockRestore();
      }
    },
  );

  it("shows the queue, elapsed time and latest output on the running rows", async () => {
    const claudeRun = deferred<void>();
    const codexRun = deferred<void>();
    mocks.runToolLifecycleAction.mockImplementation(
      async ([tool]: string[]) => {
        await (tool === "claude" ? claudeRun : codexRun).promise;
        upgraded.add(tool);
      },
    );
    await renderApps();
    fireEvent.click(updateButton("Claude Code"));
    fireEvent.click(updateButton("Codex"));
    await waitFor(() =>
      expect(mocks.runToolLifecycleAction).toHaveBeenCalledTimes(2),
    );

    const progress = (tool: string, stage: string, line?: string) =>
      emitTauriEvent("tool-lifecycle-progress", { tools: [tool], stage, line });
    act(() => {
      progress("claude", "running");
      progress("claude", "output", "==> [4/5] Installing");
      progress("codex", "queued");
      // 没在执行的工具收到迟到的事件，不显示进度
      progress("gemini", "output", "stray line");
    });
    expect(card("Claude Code").getByText("appsPage.toolElapsed")).toBeVisible();
    expect(card("Claude Code").getByText("==> [4/5] Installing")).toBeVisible();
    expect(card("Codex").getByText("appsPage.toolQueued")).toBeVisible();
    expect(card("Gemini CLI").queryByText("stray line")).toBeNull();

    await act(async () => claudeRun.resolve());
    await waitFor(() => expect(isReady("Claude Code")).toBe(true));
    expect(card("Claude Code").queryByText("==> [4/5] Installing")).toBeNull();
    expect(card("Codex").getByText("appsPage.toolQueued")).toBeVisible();

    act(() => progress("codex", "running"));
    expect(card("Codex").queryByText("appsPage.toolQueued")).toBeNull();
    expect(card("Codex").getByText("appsPage.toolElapsed")).toBeVisible();
    await act(async () => codexRun.resolve());
    await waitFor(() => expect(isReady("Codex")).toBe(true));
  });

  it("keeps a tool locked through version refresh and remount with an expired cache", async () => {
    const refreshed =
      deferred<Awaited<ReturnType<typeof mocks.getToolVersions>>>();
    const view = await renderApps();
    mocks.getToolVersions.mockImplementationOnce(() => refreshed.promise);
    fireEvent.click(updateButton("Claude Code"));
    await waitFor(() =>
      expect(
        card("Claude Code").getAllByLabelText("common.loading").length,
      ).toBeGreaterThan(0),
    );
    // 等原任务的版本刷新真正发出（初始探测 + 刷新共 2 次）再卸载，
    // 否则机器慢时刷新可能落在重新挂载之后，计数就会抖
    await waitFor(() =>
      expect(
        mocks.getToolVersions.mock.calls.filter(([tools]) =>
          tools.includes("claude"),
        ),
      ).toHaveLength(2),
    );
    view.unmount();
    const now = vi
      .spyOn(Date, "now")
      .mockReturnValue(Date.now() + 11 * 60 * 1000);
    try {
      await renderApps();
      // Initial probe and the original task's refresh only: remount must not
      // probe a tool again while its installation/version refresh is pending.
      expect(
        mocks.getToolVersions.mock.calls.filter(([tools]) =>
          tools.includes("claude"),
        ),
      ).toHaveLength(2);
    } finally {
      now.mockRestore();
    }
    expect(updateButton("Claude Code")).toBeDisabled();
    expect(updateButton("Claude Code")).toHaveAttribute("aria-busy", "true");
    // 缓存过期，重新挂载时别的工具会重新探测版本；等探测完、按钮出来再点「全部更新」，
    // 不然机器慢时这几行还在加载
    await waitFor(() => {
      expect(updateButton("Codex")).toBeEnabled();
      expect(updateButton("Gemini CLI")).toBeEnabled();
    });
    fireEvent.click(updateAllButton());
    await waitFor(() =>
      expect(mocks.runToolLifecycleAction).toHaveBeenCalledTimes(3),
    );
    expect(
      mocks.runToolLifecycleAction.mock.calls.filter(([tools]) =>
        tools.includes("claude"),
      ),
    ).toHaveLength(1);
    await act(async () =>
      refreshed.resolve([
        {
          name: "claude",
          version: "2.0.0",
          latest_version: "2.0.0",
          error: null,
          installed_but_broken: false,
          env_type: "windows",
          wsl_distro: null,
        },
      ]),
    );
    expect(isReady("Claude Code")).toBe(true);
  });

  it("releases skipped unmanaged tools and runs the remaining batch", async () => {
    mocks.probeToolInstallations.mockImplementation(async (tools: string[]) =>
      tools.map((tool) => report(tool, { unmanaged: tool === "claude" })),
    );
    await renderApps();
    fireEvent.click(updateAllButton());
    await waitFor(() => expect(isReady("Codex")).toBe(true));
    expect(isReady("Gemini CLI")).toBe(true);
    expect(updateButton("Claude Code")).toBeEnabled();
    expect(
      mocks.runToolLifecycleAction.mock.calls.map(([tools]) => tools),
    ).toEqual([["codex"], ["gemini"]]);
    expect(
      screen.getByRole("button", { name: "appsPage.checkUpdates" }),
    ).toBeEnabled();
    expect(mocks.warning).toHaveBeenCalledWith(
      "settings.toolUpgradeUnmanagedTitle",
      expect.anything(),
    );
  });
});
describe("AppsPage OpenCode Desktop bundled CLI", () => {
  beforeEach(() => {
    outdated.add("opencode");
  });

  /** 安装概览：OpenCode 命令行默认用的是 Desktop 自带的那份，可选再带一份独立安装。 */
  function listWithDesktopOpenCode(standaloneIsDefault = false) {
    mocks.listToolInstallations.mockImplementation(async (tools: string[]) =>
      tools.map((tool) => ({
        tool,
        installs:
          tool === "opencode"
            ? [
                {
                  path: "C:/Program Files/OpenCode/opencode-cli.exe",
                  source: "desktop",
                  is_path_default: !standaloneIsDefault,
                },
                ...(standaloneIsDefault
                  ? [
                      {
                        path: "C:/npm/opencode.cmd",
                        source: "system",
                        is_path_default: true,
                      },
                    ]
                  : []),
              ]
            : [
                {
                  path: `C:/npm/${tool}.cmd`,
                  source: "system",
                  is_path_default: true,
                },
              ],
        is_wsl: false,
      })),
    );
  }

  it("does not offer a separate upgrade for the CLI bundled with OpenCode Desktop", async () => {
    listWithDesktopOpenCode();
    await renderApps();
    await waitFor(() =>
      expect(
        card("OpenCode").getByText("settings.toolUpgradeDesktopManagedDetail"),
      ).toBeInTheDocument(),
    );
    expect(
      card("OpenCode").queryByRole("button", { name: /settings\.toolUpdate/ }),
    ).toBeNull();
    expect(card("OpenCode").getByText("OpenCode Desktop")).toBeInTheDocument();

    // 「全部更新」只预检其余过期的工具
    fireEvent.click(updateAllButton());
    await waitFor(() =>
      expect(mocks.probeToolInstallations).toHaveBeenCalledWith([
        "claude",
        "codex",
        "gemini",
      ]),
    );
  });

  it("still upgrades a standalone OpenCode that the command line uses", async () => {
    listWithDesktopOpenCode(true);
    await renderApps();
    await waitFor(() => expect(updateButton("OpenCode")).toBeEnabled());
    expect(
      card("OpenCode").queryByText("settings.toolUpgradeDesktopManagedDetail"),
    ).toBeNull();
  });

  it("keeps the sidebar dot off for a Desktop-managed OpenCode after the background check", async () => {
    listWithDesktopOpenCode();
    outdated.clear();
    outdated.add("opencode");
    const { checkToolUpdatesInBackground, useToolUpdatesAvailable } =
      await import("@/components/apps/useToolManagement");
    const { result } = renderHook(() => useToolUpdatesAvailable());

    await act(async () => {
      await checkToolUpdatesInBackground();
    });

    // OpenCode 过期时后台检查顺带拿安装概览，认出是 Desktop 自带的就不亮圆点
    expect(mocks.listToolInstallations).toHaveBeenCalledTimes(1);
    expect(result.current).toBe(false);
  });
});
