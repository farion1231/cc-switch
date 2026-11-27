import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  mobileApi,
  type RemoteAgentsResponse,
  type RemoteRunSnapshot,
  type SafeRemoteSession,
} from "@/mobile/api";
import { RemoteControlView } from "@/mobile/views/RemoteControlView";

const MOCK_AGENTS: RemoteAgentsResponse = {
  agents: [
    {
      id: "agy",
      installed: true,
      enabled: true,
      supportsResume: true,
      supportsModel: true,
      supportsEffort: true,
      supportsMode: true,
      supportsSandbox: true,
      modes: ["plan", "accept-edits", "auto"],
      efforts: ["low", "medium", "high", "xhigh", "max"],
      activeRunCount: 0,
    },
    {
      id: "claude",
      installed: true,
      enabled: false,
      supportsResume: true,
      supportsModel: true,
      supportsEffort: true,
      supportsMode: true,
      supportsSandbox: false,
      modes: ["default", "plan", "acceptEdits", "dontAsk"],
      efforts: ["low", "medium", "high", "max"],
      activeRunCount: 0,
    },
    {
      id: "copilot",
      installed: true,
      enabled: true,
      supportsResume: true,
      supportsModel: true,
      supportsEffort: false,
      supportsMode: false,
      supportsSandbox: false,
      modes: [],
      efforts: [],
      activeRunCount: 0,
    },
  ],
  defaultCwd: "/Users/dev",
  recentWorkspaces: ["/Users/dev/project-alpha", "/Users/dev/project-beta"],
};

const MOCK_SESSIONS: SafeRemoteSession[] = [
  {
    app: "agy",
    sessionId: "conv-1",
    title: "Refactor router",
    projectDir: "/Users/dev/project-alpha",
    lastActiveAt: Math.floor(Date.now() / 1000) - 120,
    resumable: true,
  },
  {
    app: "claude",
    sessionId: "claude-1",
    title: "Investigate flaky test",
    projectDir: "/Users/dev/project-beta",
    lastActiveAt: Math.floor(Date.now() / 1000) - 300,
    resumable: true,
  },
];

describe("RemoteControlView", () => {
  beforeEach(() => {
    vi.spyOn(mobileApi, "getRemoteAgents").mockResolvedValue(MOCK_AGENTS);
    vi.spyOn(mobileApi, "getRemoteSessions").mockResolvedValue({
      sessions: MOCK_SESSIONS,
    });
    vi.spyOn(mobileApi, "getRemoteRuns").mockResolvedValue({
      activeRunCount: 0,
      runs: [],
    });
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("renders agent filters, session cards, and read-only notice for disabled agent", async () => {
    vi.spyOn(mobileApi, "getRemoteSessionMessages").mockResolvedValue({
      session: MOCK_SESSIONS[1],
      messages: [
        { role: "user", blocks: [{ type: "text", text: "Check CI failure" }] },
        {
          role: "assistant",
          blocks: [{ type: "text", text: "Found race condition" }],
        },
      ],
    });

    render(
      <RemoteControlView lang="en" refreshSignal={0} onAuthExpired={vi.fn()} />,
    );

    expect(await screen.findByText("Refactor router")).toBeInTheDocument();
    expect(screen.getByText("Investigate flaky test")).toBeInTheDocument();

    // Open disabled Claude session -> read-only notice shown, no composer
    fireEvent.click(screen.getByText("Investigate flaky test"));
    expect(await screen.findByText("Found race condition")).toBeInTheDocument();
    expect(
      screen.getByText(/Remote execution is disabled for this agent/i),
    ).toBeInTheDocument();
    expect(
      screen.queryByPlaceholderText("Send a message to the agent…"),
    ).not.toBeInTheDocument();
  });

  it("shows dynamic per-agent controls in New Session sheet and starts/cancels a live run", async () => {
    const runningSnapshot: RemoteRunSnapshot = {
      runId: "run-99",
      app: "agy",
      status: "running",
      prompt: "Run checks",
      cwd: "/Users/dev/project-alpha",
      mode: "plan",
      sandbox: false,
      createdAt: Date.now(),
      updatedAt: Date.now(),
      lastSeq: 1,
    };
    const startSpy = vi
      .spyOn(mobileApi, "startRemoteRun")
      .mockResolvedValue(runningSnapshot);
    const cancelSpy = vi.spyOn(mobileApi, "cancelRemoteRun").mockResolvedValue({
      ...runningSnapshot,
      status: "cancelled",
    });
    vi.spyOn(mobileApi, "getRemoteRun").mockResolvedValue({
      run: { ...runningSnapshot, status: "cancelled" },
      events: [
        { seq: 2, ts: Date.now(), kind: "thought", text: "Analyzing repo" },
        { seq: 3, ts: Date.now(), kind: "text", text: "Partial output" },
        { seq: 4, ts: Date.now(), kind: "cancelled" },
      ],
    });

    render(
      <RemoteControlView lang="en" refreshSignal={0} onAuthExpired={vi.fn()} />,
    );

    await screen.findByText("Refactor router");

    // Open New Session sheet
    fireEvent.click(screen.getByRole("button", { name: /New Session/i }));
    const dialog = screen.getByRole("dialog", { name: "New Session" });
    expect(dialog).toBeInTheDocument();

    // Antigravity supports Mode, Effort, and Sandbox
    expect(within(dialog).getByText("Sandbox")).toBeInTheDocument();
    expect(within(dialog).getByText("Effort")).toBeInTheDocument();

    // Switching agent to Copilot hides Effort, Mode, and Sandbox dynamically
    fireEvent.click(within(dialog).getByRole("button", { name: "Copilot" }));
    expect(within(dialog).queryByText("Sandbox")).not.toBeInTheDocument();
    expect(within(dialog).queryByText("Effort")).not.toBeInTheDocument();

    // Switch back to Antigravity and start a run
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Antigravity" }),
    );
    fireEvent.change(
      within(dialog).getByPlaceholderText("Send a message to the agent…"),
      { target: { value: "Run checks" } },
    );
    fireEvent.click(within(dialog).getByRole("button", { name: /Start Run/i }));

    await waitFor(() => {
      expect(startSpy).toHaveBeenCalledWith(
        expect.objectContaining({
          app: "agy",
          prompt: "Run checks",
          cwd: "/Users/dev/project-alpha",
        }),
      );
    });

    // Stop button cancels the active run
    const stopButtons = await screen.findAllByRole("button", { name: /Stop/i });
    fireEvent.click(stopButtons[0]);
    await waitFor(() => {
      expect(cancelSpy).toHaveBeenCalledWith("run-99");
    });
  });
});
