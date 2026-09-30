import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { settingsApi, ToolInstallationReport } from "@/lib/api/settings";

type ToolVersions = Awaited<ReturnType<typeof settingsApi.getToolVersions>>;

const mocks = vi.hoisted(() => ({
  getToolVersions: vi.fn(),
  probeToolInstallations: vi.fn(),
  runToolLifecycleAction: vi.fn(),
  info: vi.fn(),
  success: vi.fn(),
  warning: vi.fn(),
  error: vi.fn(),
}));

vi.mock("@/lib/api", () => ({ settingsApi: mocks }));
vi.mock("@tauri-apps/api/app", () => ({ getVersion: async () => "3.20.4" }));
vi.mock("@/contexts/UpdateContext", () => ({
  useUpdate: () => ({ hasUpdate: false, isChecking: false }),
}));
vi.mock("@/config/appConfig", () => ({ APP_ICON_MAP: {} }));
vi.mock("sonner", () => ({ toast: mocks }));

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

function report(
  tool: string,
  overrides: Partial<ToolInstallationReport> = {},
): ToolInstallationReport {
  return {
    tool,
    installs: [],
    is_conflict: false,
    needs_confirmation: false,
    command: `${tool} update`,
    anchored: true,
    unmanaged: false,
    ...overrides,
  };
}

const upgraded = new Set<string>();
const outdated = new Set<string>();
const missing = new Set<string>();

function card(name: string) {
  return within(screen.getByText(name).closest(".rounded-xl") as HTMLElement);
}

function updateButton(name: string) {
  return card(name).getByRole("button", { name: "settings.toolUpdate" });
}

async function renderAbout() {
  // AboutSection caches version results at module scope between mounts.
  const { AboutSection } = await import("@/components/settings/AboutSection");
  const view = render(<AboutSection isPortable={false} />);
  await waitFor(() =>
    expect(
      within(view.container).getByText("common.refresh"),
    ).toBeInTheDocument(),
  );
  return view;
}

describe("AboutSection concurrent CLI upgrades", () => {
  beforeEach(() => {
    vi.resetModules();
    upgraded.clear();
    outdated.clear();
    missing.clear();
    outdated.add("claude").add("codex").add("gemini");
    mocks.getToolVersions
      .mockReset()
      .mockImplementation(async (tools: string[]) =>
        tools.map((name) => ({
          name,
          version: missing.has(name)
            ? null
            : upgraded.has(name) || !outdated.has(name)
              ? "2.0.0"
              : "1.0.0",
          latest_version: "2.0.0",
          error: null,
          installed_but_broken: false,
          env_type: "windows",
          wsl_distro: null,
        })),
      );
    mocks.probeToolInstallations
      .mockReset()
      .mockImplementation(async (tools: string[]) =>
        tools.map((tool) => report(tool)),
      );
    mocks.runToolLifecycleAction
      .mockReset()
      .mockImplementation(async ([tool]: string[]) => {
        upgraded.add(tool);
        missing.delete(tool);
      });
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
    await renderAbout();

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
    expect(card("Codex").getByText("settings.toolReady")).toBeInTheDocument();
    expect(updateButton("Claude Code")).toBeDisabled();
    await act(async () => claudeRun.resolve());
    expect(
      card("Claude Code").getByText("settings.toolReady"),
    ).toBeInTheDocument();
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
    await renderAbout();
    fireEvent.click(updateButton("Claude Code"));
    await waitFor(() =>
      expect(mocks.runToolLifecycleAction).toHaveBeenCalledTimes(1),
    );
    const updateAll = screen.getByRole("button", {
      name: "settings.updateAllTools",
    });
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
    expect(
      card("Gemini CLI").getByText("settings.toolReady"),
    ).toBeInTheDocument();
    expect(card("Codex").getByText("settings.toolReady")).toBeInTheDocument();
    expect(updateButton("Claude Code")).toBeDisabled();
    await act(async () => runs.get("claude")!.resolve());
  });

  it.each(["install", "update"] as const)(
    "restores an ongoing %s after remount and receives its completion",
    async (action) => {
      if (action === "install") missing.add("claude");
      const running = deferred<void>();
      mocks.runToolLifecycleAction.mockImplementationOnce(async () => {
        await running.promise;
        upgraded.add("claude");
        missing.delete("claude");
      });
      const actionButton = () =>
        card("Claude Code").getByRole("button", {
          name:
            action === "install"
              ? "settings.toolInstall"
              : "settings.toolUpdate",
        });
      const view = await renderAbout();
      fireEvent.click(actionButton());
      await waitFor(() =>
        expect(mocks.runToolLifecycleAction).toHaveBeenCalledTimes(1),
      );
      view.unmount();
      await renderAbout();
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
      expect(
        card("Claude Code").getByText("settings.toolReady"),
      ).toBeInTheDocument();
      expect(
        screen.getByRole("button", { name: "common.refresh" }),
      ).toBeEnabled();
      expect(mocks.success).toHaveBeenCalledTimes(1);
    },
  );

  it.each(["install", "update"] as const)(
    "shows an informational toast when the backend reports an ongoing %s",
    async (action) => {
      if (action === "install") missing.add("claude");
      mocks.runToolLifecycleAction.mockRejectedValueOnce(
        "TOOL_ACTION_IN_PROGRESS",
      );
      await renderAbout();
      const versionChecks = mocks.getToolVersions.mock.calls.length;
      fireEvent.click(
        card("Claude Code").getByRole("button", {
          name:
            action === "install"
              ? "settings.toolInstall"
              : "settings.toolUpdate",
        }),
      );
      await waitFor(() =>
        expect(mocks.info).toHaveBeenCalledWith(
          "settings.toolActionInProgress",
          {
            description: "settings.toolActionInProgressDetail",
            closeButton: true,
          },
        ),
      );
      expect(mocks.getToolVersions).toHaveBeenCalledTimes(versionChecks);
      expect(mocks.error).not.toHaveBeenCalled();
      expect(mocks.warning).not.toHaveBeenCalled();
      expect(mocks.success).not.toHaveBeenCalled();
    },
  );

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
    const { AboutSection } = await import("@/components/settings/AboutSection");
    const view = render(<AboutSection isPortable={false} />);
    await waitFor(() => expect(mocks.getToolVersions).toHaveBeenCalledTimes(9));
    view.unmount();
    const remounted = await renderAbout();
    fireEvent.click(updateButton("Claude Code"));
    await waitFor(() =>
      expect(
        card("Claude Code").queryByText("settings.toolReady"),
      ).not.toBeNull(),
    );

    await act(async () => stale.resolve(oldResult));
    expect(
      card("Claude Code").queryByText("settings.toolReady"),
    ).not.toBeNull();
    expect(card("Claude Code").queryByText("1.0.0")).not.toBeInTheDocument();
    remounted.unmount();
    await renderAbout();
    expect(
      card("Claude Code").getByText("settings.toolReady"),
    ).toBeInTheDocument();
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
      const view = await renderAbout();
      await user.click(card("Claude Code").getAllByRole("combobox")[0]);
      await user.click(screen.getByRole("option", { name: "bash" }));
      await waitFor(() => expect(updateButton("Claude Code")).toBeEnabled());
      await user.click(card("Claude Code").getAllByRole("combobox")[1]);
      await user.click(screen.getByRole("option", { name: "-lic" }));
      await waitFor(() => expect(updateButton("Claude Code")).toBeEnabled());
      fireEvent.click(
        screen.getByRole("button", { name: "settings.updateAllTools" }),
      );
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
      await renderAbout();
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
    const view = await renderAbout();
    fireEvent.click(
      screen.getByRole("button", { name: "settings.updateAllTools" }),
    );
    await waitFor(() =>
      expect(mocks.runToolLifecycleAction).toHaveBeenCalledTimes(3),
    );
    view.unmount();
    await act(async () => runs.get("codex")!.resolve());
    await renderAbout();
    const updateAll = screen.getByRole("button", {
      name: "settings.updateAllTools",
    });
    expect(updateAll).toBeDisabled();
    expect(updateAll).toHaveAttribute("aria-busy", "true");
    expect(card("Codex").getByText("settings.toolReady")).toBeInTheDocument();
    expect(updateButton("Claude Code")).toHaveAttribute("aria-busy", "true");
    expect(updateButton("Gemini CLI")).toBeDisabled();

    await act(async () => runs.get("gemini")!.resolve());
    expect(
      card("Gemini CLI").getByText("settings.toolReady"),
    ).toBeInTheDocument();
    expect(updateButton("Claude Code")).toBeDisabled();
    expect(updateAll).toHaveAttribute("aria-busy", "true");
    await act(async () => runs.get("claude")!.resolve());
    expect(
      card("Claude Code").getByText("settings.toolReady"),
    ).toBeInTheDocument();
    expect(updateAll).toHaveAttribute("aria-busy", "false");
    expect(
      screen.getByRole("button", { name: "common.refresh" }),
    ).toBeEnabled();
    expect(mocks.runToolLifecycleAction).toHaveBeenCalledTimes(3);
    expect(mocks.success).toHaveBeenCalledTimes(1);
  });

  it("preserves preflight and its confirmation when navigating away and back", async () => {
    const preflight = deferred<ToolInstallationReport[]>();
    mocks.probeToolInstallations.mockImplementationOnce(
      () => preflight.promise,
    );
    const view = await renderAbout();
    fireEvent.click(updateButton("Claude Code"));
    view.unmount();
    const remounted = await renderAbout();
    expect(updateButton("Claude Code")).toBeDisabled();
    expect(updateButton("Claude Code")).toHaveAttribute("aria-busy", "true");
    expect(mocks.probeToolInstallations).toHaveBeenCalledTimes(1);
    await act(async () =>
      preflight.resolve([report("claude", { needs_confirmation: true })]),
    );
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    remounted.unmount();
    await renderAbout();
    expect(
      within(screen.getByRole("dialog")).getByText("Claude Code"),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "common.cancel" }));
    expect(updateButton("Claude Code")).toBeEnabled();
    expect(mocks.runToolLifecycleAction).not.toHaveBeenCalled();
    fireEvent.click(updateButton("Claude Code"));
    await waitFor(() =>
      expect(
        card("Claude Code").getByText("settings.toolReady"),
      ).toBeInTheDocument(),
    );
  });

  it("unlocks a failed background task after remount so it can be retried", async () => {
    const running = deferred<void>();
    mocks.runToolLifecycleAction.mockImplementationOnce(() => running.promise);
    const errorLog = vi.spyOn(console, "error").mockImplementation(() => {});
    try {
      const view = await renderAbout();
      fireEvent.click(updateButton("Claude Code"));
      await waitFor(() =>
        expect(mocks.runToolLifecycleAction).toHaveBeenCalledTimes(1),
      );
      view.unmount();
      await renderAbout();
      expect(updateButton("Claude Code")).toBeDisabled();
      await act(async () => running.reject(new Error("installer failed")));
      expect(updateButton("Claude Code")).toBeEnabled();
      expect(mocks.error).toHaveBeenCalledWith("settings.toolActionFailed", {
        description: "installer failed",
        closeButton: true,
      });
      fireEvent.click(updateButton("Claude Code"));
      await waitFor(() =>
        expect(
          card("Claude Code").getByText("settings.toolReady"),
        ).toBeInTheDocument(),
      );
    } finally {
      errorLog.mockRestore();
    }
  });

  it("does not report success or failure when all requested tools are already running", async () => {
    mocks.runToolLifecycleAction.mockRejectedValue("TOOL_ACTION_IN_PROGRESS");
    await renderAbout();
    const versionChecks = mocks.getToolVersions.mock.calls.length;
    fireEvent.click(
      screen.getByRole("button", { name: "settings.updateAllTools" }),
    );
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
    await renderAbout();
    fireEvent.click(
      screen.getByRole("button", { name: "settings.updateAllTools" }),
    );
    await waitFor(() => expect(mocks.success).toHaveBeenCalledTimes(1));
    expect(mocks.info).toHaveBeenCalledTimes(1);
    expect(card("Codex").getByText("settings.toolReady")).toBeInTheDocument();
    expect(
      card("Gemini CLI").getByText("settings.toolReady"),
    ).toBeInTheDocument();
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
      await renderAbout();
      fireEvent.click(
        screen.getByRole("button", { name: "settings.updateAllTools" }),
      );
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
    await renderAbout();
    fireEvent.click(
      screen.getByRole("button", { name: "settings.updateAllTools" }),
    );
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
    expect(
      card("Claude Code").getByText("settings.toolReady"),
    ).toBeInTheDocument();
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
    await renderAbout();
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
      await renderAbout();
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
    await renderAbout();
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

  it("does not block another installation submission while an upgrade is running", async () => {
    const claudeRun = deferred<void>();
    missing.add("codex");
    mocks.runToolLifecycleAction.mockImplementation(
      async ([tool]: string[]) => {
        if (tool === "claude") await claudeRun.promise;
        upgraded.add(tool);
        missing.delete(tool);
      },
    );
    await renderAbout();
    fireEvent.click(updateButton("Claude Code"));
    await waitFor(() =>
      expect(mocks.runToolLifecycleAction).toHaveBeenCalledTimes(1),
    );
    const install = card("Codex").getByRole("button", {
      name: "settings.toolInstall",
    });
    expect(install).toBeEnabled();
    fireEvent.click(install);
    await waitFor(() =>
      expect(card("Codex").getByText("settings.toolReady")).toBeInTheDocument(),
    );
    expect(mocks.runToolLifecycleAction).toHaveBeenCalledWith(
      ["codex"],
      "install",
      {},
    );
    expect(updateButton("Claude Code")).toBeDisabled();
    await act(async () => claudeRun.resolve());
  });

  it("keeps a tool locked through version refresh and remount with an expired cache", async () => {
    const refreshed =
      deferred<Awaited<ReturnType<typeof mocks.getToolVersions>>>();
    const view = await renderAbout();
    mocks.getToolVersions.mockImplementationOnce(() => refreshed.promise);
    fireEvent.click(updateButton("Claude Code"));
    await waitFor(() =>
      expect(
        card("Claude Code").getAllByText("common.loading").length,
      ).toBeGreaterThan(0),
    );
    view.unmount();
    const now = vi
      .spyOn(Date, "now")
      .mockReturnValue(Date.now() + 11 * 60 * 1000);
    try {
      await renderAbout();
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
    expect(updateButton("Codex")).toBeEnabled();
    fireEvent.click(
      screen.getByRole("button", { name: "settings.updateAllTools" }),
    );
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
    expect(
      card("Claude Code").getByText("settings.toolReady"),
    ).toBeInTheDocument();
  });

  it("releases skipped unmanaged tools and runs the remaining batch", async () => {
    mocks.probeToolInstallations.mockImplementation(async (tools: string[]) =>
      tools.map((tool) => report(tool, { unmanaged: tool === "claude" })),
    );
    await renderAbout();
    fireEvent.click(
      screen.getByRole("button", { name: "settings.updateAllTools" }),
    );
    await waitFor(() =>
      expect(card("Codex").getByText("settings.toolReady")).toBeInTheDocument(),
    );
    expect(
      card("Gemini CLI").getByText("settings.toolReady"),
    ).toBeInTheDocument();
    expect(updateButton("Claude Code")).toBeEnabled();
    expect(
      mocks.runToolLifecycleAction.mock.calls.map(([tools]) => tools),
    ).toEqual([["codex"], ["gemini"]]);
    expect(
      screen.getByRole("button", { name: "common.refresh" }),
    ).toBeEnabled();
    expect(mocks.warning).toHaveBeenCalledWith(
      "settings.toolUpgradeUnmanagedTitle",
      expect.anything(),
    );
  });
});
