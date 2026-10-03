import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { HermesCapturePanel } from "@/components/usage/HermesCapturePanel";

const getEvents = vi.hoisted(() => vi.fn());
const getHistory = vi.hoisted(() => vi.fn());
const replayHistory = vi.hoisted(() => vi.fn());
const enableCapture = vi.hoisted(() => vi.fn());

vi.mock("@/lib/api/usage", () => ({
  usageApi: {
    getHermesRequestEvents: getEvents,
    getHermesHistoryEstimates: getHistory,
    replayHermesHistory: replayHistory,
    enableHermesCapturePlugin: enableCapture,
  },
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
    i18n: { language: "en" },
  }),
}));

vi.mock("sonner", () => ({
  toast: { success: vi.fn(), warning: vi.fn(), error: vi.fn() },
}));

function mount() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <HermesCapturePanel
        startMs={1}
        endMs={200000}
        profileName="profile-a"
        refreshIntervalMs={0}
      />
    </QueryClientProvider>,
  );
}

describe("HermesCapturePanel", () => {
  beforeEach(() => {
    getEvents.mockReset().mockResolvedValue([
      {
        eventId: "main:one:success",
        profileName: "profile-a",
        kind: "main",
        provider: "provider-a",
        model: "model-a",
        startedAtMs: 100000,
        status: "success",
        statusCode: null,
        durationMs: 1000,
        usageAvailable: true,
        inputTokens: 3,
        outputTokens: 4,
      },
    ]);
    getHistory.mockReset().mockResolvedValue([
      {
        profileName: "profile-a",
        capturedAtMs: 90000,
        requestCount: 2,
        inputTokens: 10,
        outputTokens: 20,
        cacheReadTokens: 0,
        cacheWriteTokens: 0,
        reasoningTokens: 0,
        costUsd: "0.25",
      },
    ]);
    replayHistory.mockReset().mockResolvedValue({
      imported: 0,
      skipped: 1,
      unavailable: 0,
      errors: [],
    });
    enableCapture.mockReset().mockResolvedValue("profile-a");
  });

  it("lists exact rows separately from the historical estimate and scopes activation", async () => {
    mount();
    expect(await screen.findByText("provider-a / model-a")).toBeInTheDocument();
    expect(getEvents).toHaveBeenCalledWith({
      startMs: 1,
      endMs: 200000,
      profileName: "profile-a",
      task: undefined,
      providerName: undefined,
      model: undefined,
      offset: 0,
    });
    expect(
      screen.getByText("usage.hermes.historyEstimate"),
    ).toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("button", { name: "usage.hermes.enableCapture" }),
    );
    await waitFor(() =>
      expect(enableCapture).toHaveBeenCalledWith("profile-a"),
    );
    fireEvent.click(
      screen.getByRole("button", { name: "usage.hermes.replayHistory" }),
    );
    await waitFor(() => expect(replayHistory).toHaveBeenCalledTimes(1));
  });

  it("can request the next page of captured events", async () => {
    getEvents
      .mockResolvedValueOnce(
        Array.from({ length: 100 }, (_, index) => ({
          eventId: `event-${index}`,
          profileName: "profile-a",
          kind: "main",
          provider: "provider-a",
          model: "model-a",
          startedAtMs: index + 1,
          status: "success",
          usageAvailable: false,
        })),
      )
      .mockResolvedValueOnce([]);
    mount();
    const next = await screen.findByRole("button", {
      name: "usage.hermes.nextPage",
    });
    await waitFor(() => expect(next).toBeEnabled());
    fireEvent.click(next);
    await waitFor(() =>
      expect(getEvents).toHaveBeenCalledWith(
        expect.objectContaining({ offset: 100 }),
      ),
    );
  });

  it("distinguishes main cumulative elapsed time from auxiliary attempt elapsed time", async () => {
    getEvents.mockResolvedValue([
      {
        eventId: "main:r:error:2",
        profileName: "profile-a",
        kind: "main",
        startedAtMs: 100000,
        provider: "p",
        model: "m",
        status: "error",
        durationMs: 9488,
        usageAvailable: false,
      },
      {
        eventId: "aux:r:0",
        profileName: "profile-a",
        kind: "aux",
        auxTask: "approval",
        startedAtMs: 100000,
        provider: "p",
        model: "m",
        status: "success",
        durationMs: 1063,
        usageAvailable: false,
      },
      {
        eventId: "main:unknown:success",
        profileName: "profile-a",
        kind: "main",
        startedAtMs: 100000,
        provider: "p",
        model: "m",
        status: "success",
        durationMs: null,
        usageAvailable: false,
      },
    ]);
    mount();
    expect(await screen.findByText("9488 ms")).toBeInTheDocument();
    expect(screen.getByText("usage.hermes.mainElapsed")).toBeInTheDocument();
    expect(screen.getByText("1063 ms")).toBeInTheDocument();
    expect(
      screen.getByText("usage.hermes.auxAttemptElapsed"),
    ).toBeInTheDocument();
    expect(screen.getByText("—")).toBeInTheDocument();
  });

  it("shows the capture plugin compatibility requirement", () => {
    mount();
    expect(
      screen.getByText("usage.hermes.captureCompatibility"),
    ).toBeInTheDocument();
  });
});
