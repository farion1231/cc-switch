import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { SessionUsageSummary } from "@/components/sessions/SessionUsageSummary";
import {
  createAgentSessionUsageSummary as summary,
  createAgentUsageMeasure as measure,
  createAgentUsageSourceDimension as sourceDimension,
} from "../fixtures/agentUsage";

const useAgentSessionUsageMock = vi.hoisted(() => vi.fn());

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: string | { defaultValue?: string }) =>
      typeof options === "string" ? options : (options?.defaultValue ?? key),
    i18n: {
      resolvedLanguage: "en-US",
      language: "en-US",
    },
  }),
}));

vi.mock("@/lib/query/usage", async () => {
  const actual =
    await vi.importActual<typeof import("@/lib/query/usage")>(
      "@/lib/query/usage",
    );
  return {
    ...actual,
    useAgentSessionUsage: (...args: unknown[]) =>
      useAgentSessionUsageMock(...args),
  };
});

const renderSummary = (sessionId = "root-session") =>
  render(<SessionUsageSummary appType="codex" sessionId={sessionId} />);

const installSummary = (overrides: Parameters<typeof summary>[0] = {}) =>
  useAgentSessionUsageMock.mockReturnValue({
    data: summary(overrides),
    isLoading: false,
    isError: false,
  });

describe("SessionUsageSummary", () => {
  beforeEach(() => {
    useAgentSessionUsageMock.mockReset();
    installSummary();
  });

  it("uses the optional canonical usage identity for its query", () => {
    render(
      <SessionUsageSummary
        appType="hermes"
        sessionId="raw-hermes-session"
        usageSessionId="hermes:default:database:digest"
      />,
    );

    expect(useAgentSessionUsageMock).toHaveBeenCalledWith(
      "hermes",
      "hermes:default:database:digest",
    );
  });

  it("shows self, descendant aggregate, API-derived total, and count without child rows", () => {
    const selfUsage = measure({ inputTokens: 10, outputTokens: 5 });
    const descendantUsage = measure({
      inputTokens: 20,
      outputTokens: 5,
      requestCount: 3,
      requestCountSemantics: "agent_call",
    });
    const totalUsage = measure({
      inputTokens: 30,
      outputTokens: 10,
      requestCount: 5,
      requestCountSemantics: "agent_call",
      totalCostUsd: "0.03",
    });
    installSummary({
      supportsDescendants: true,
      selfUsage,
      descendantUsage,
      totalUsage,
      descendantSessionCount: 2,
    });

    renderSummary();

    expect(screen.getByText("Task total")).toBeInTheDocument();
    expect(screen.getByTestId("session-usage-total-tokens")).toHaveTextContent(
      "40",
    );
    expect(screen.getByText("This task")).toBeInTheDocument();
    expect(screen.getByText("All descendants (2)")).toBeInTheDocument();
    expect(screen.getAllByText(/agent calls/).length).toBeGreaterThan(0);
    expect(screen.queryByText("Request-exact")).not.toBeInTheDocument();
    expect(screen.queryByText("Precision")).not.toBeInTheDocument();
    expect(screen.queryByTestId("usage-precision")).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Data details" }),
    ).not.toBeInTheDocument();
    expect(screen.queryByText(/descendant-1|child-1/i)).not.toBeInTheDocument();
  });

  it("hides the descendant aggregate for a self-only source", () => {
    renderSummary();

    expect(screen.getByText("This task")).toBeInTheDocument();
    expect(screen.queryByText("All descendants")).not.toBeInTheDocument();
  });

  it("explains that Codex costs are withheld while canonical replay is running", async () => {
    const replaying = measure({ totalCostUsd: null, partial: true });
    installSummary({
      selfUsage: replaying,
      partial: true,
      sourceDimensions: [
        sourceDimension("codex", "codex_session", {
          costStatus: "unavailable",
          costSource: "codex_replay",
        }),
      ],
    });

    renderSummary("replaying-session");

    expect(screen.getAllByText("Unavailable").length).toBeGreaterThan(0);
    expect(
      screen.queryByText(
        "Codex history is still being replayed; cost will appear when replay completes.",
      ),
    ).not.toBeInTheDocument();
    const warning = screen.getByRole("img", {
      name: /Codex history is still being replayed/,
    });
    fireEvent.focus(warning);
    await waitFor(() =>
      expect(
        screen.getAllByText(
          "Codex history is still being replayed; cost will appear when replay completes.",
        ).length,
      ).toBeGreaterThan(0),
    );
  });

  it("keeps unavailable and partial values distinct from explicit zero and marks sync windows", async () => {
    const partialSyncMeasure = measure({
      requestCount: null,
      inputTokens: 12,
      outputTokens: 4,
      cacheCreationTokens: null,
      totalCostUsd: null,
      precision: "sync_window_delta",
      timeSemantics: "sync_window_end",
      requestCountSemantics: "unavailable",
      partial: true,
    });
    installSummary({
      selfUsage: partialSyncMeasure,
      partial: true,
    });

    const view = renderSummary();

    expect(screen.getByTestId("session-usage-total-tokens")).toHaveTextContent(
      "16+",
    );
    expect(screen.getAllByText("Unavailable").length).toBeGreaterThan(0);
    expect(screen.queryByText("Partial")).not.toBeInTheDocument();
    expect(screen.queryByText("Sync-window delta")).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Data details" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByText("Some usage fields are partial or unavailable."),
    ).not.toBeInTheDocument();
    const warning = screen.getByRole("img", {
      name: /Some usage fields are partial or unavailable\./,
    });
    fireEvent.focus(warning);
    await waitFor(() =>
      expect(
        screen.getAllByText("Some usage fields are partial or unavailable.")
          .length,
      ).toBeGreaterThan(0),
    );
    expect(
      screen.getAllByText("Sync-window increment; not per-request.").length,
    ).toBeGreaterThan(0);
    expect(screen.queryByText(/HTTP requests/)).not.toBeInTheDocument();

    const zero = measure({
      inputTokens: 0,
      outputTokens: 0,
      cacheReadTokens: 0,
      cacheCreationTokens: 0,
      totalCostUsd: "0",
      requestCount: 0,
    });
    installSummary({ selfUsage: zero });
    view.unmount();
    renderSummary("zero-session");

    expect(screen.getAllByText("$0.0000").length).toBeGreaterThan(0);
    expect(screen.getAllByText("0").length).toBeGreaterThan(0);
  });

  it("keeps the total unavailable when every token component is unknown", async () => {
    const unknown = measure({
      inputTokens: null,
      outputTokens: null,
      cacheReadTokens: null,
      cacheCreationTokens: null,
      totalCostUsd: null,
      partial: true,
    });
    installSummary({ selfUsage: unknown, partial: true });

    renderSummary("unknown-session");

    expect(screen.getByTestId("session-usage-total-tokens")).toHaveTextContent(
      "Unavailable",
    );
    expect(
      screen.queryByRole("button", { name: "Data details" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByText("Some usage fields are partial or unavailable."),
    ).not.toBeInTheDocument();
    fireEvent.focus(
      screen.getByRole("img", {
        name: /Some usage fields are partial or unavailable\./,
      }),
    );
    await waitFor(() =>
      expect(
        screen.getAllByText("Some usage fields are partial or unavailable.")
          .length,
      ).toBeGreaterThan(0),
    );
  });

  it("does not retain the previous selection while the next query loads", () => {
    installSummary();
    const view = renderSummary("first-session");
    expect(screen.getByTestId("session-usage-summary")).toBeInTheDocument();

    useAgentSessionUsageMock.mockReturnValue({
      data: undefined,
      isLoading: true,
      isError: false,
    });
    view.rerender(
      <SessionUsageSummary appType="codex" sessionId="second-session" />,
    );

    expect(screen.getByTestId("session-usage-loading")).toBeInTheDocument();
    expect(
      screen.queryByTestId("session-usage-summary"),
    ).not.toBeInTheDocument();
  });
});
