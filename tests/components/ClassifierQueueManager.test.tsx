import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { ClassifierQueueManager } from "@/components/proxy/ClassifierQueueManager";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: vi.fn() },
}));

const setConfigMock = vi.fn().mockResolvedValue(undefined);
const addMock = vi.fn().mockResolvedValue(undefined);
const removeMock = vi.fn().mockResolvedValue(undefined);

let classifierConfig = { enabled: false, forceThinkingOff: true };
let classifierQueue: Array<{ providerId: string; providerName: string }> = [];

vi.mock("@/lib/query/classifier", () => ({
  useClassifierConfig: () => ({ data: classifierConfig }),
  useSetClassifierConfig: () => ({
    mutateAsync: setConfigMock,
    isPending: false,
  }),
  useClassifierQueue: () => ({
    data: classifierQueue,
    isLoading: false,
    error: null,
  }),
  useAvailableProvidersForClassifier: () => ({
    data: [{ id: "fast", name: "Fast Provider" }],
    isLoading: false,
  }),
  useAddToClassifierQueue: () => ({ mutateAsync: addMock, isPending: false }),
  useRemoveFromClassifierQueue: () => ({
    mutateAsync: removeMock,
    isPending: false,
  }),
}));

describe("ClassifierQueueManager", () => {
  beforeEach(() => {
    setConfigMock.mockClear();
    addMock.mockClear();
    removeMock.mockClear();
    classifierConfig = { enabled: false, forceThinkingOff: true };
    classifierQueue = [];
  });

  it("explains the fallback behaviour when the queue is empty", () => {
    render(<ClassifierQueueManager appType="claude" />);
    expect(screen.getByText("proxy.classifierQueue.empty")).toBeInTheDocument();
  });

  it("persists both switches when the master switch is toggled on", async () => {
    render(<ClassifierQueueManager appType="claude" />);

    fireEvent.click(
      screen.getByRole("switch", { name: "proxy.classifier.enable" }),
    );

    await waitFor(() =>
      expect(setConfigMock).toHaveBeenCalledWith({
        appType: "claude",
        config: { enabled: true, forceThinkingOff: true },
      }),
    );
  });

  it("disables the thinking switch while the classifier queue is off", () => {
    render(<ClassifierQueueManager appType="claude" />);

    expect(
      screen.getByRole("switch", { name: "proxy.classifier.forceThinkingOff" }),
    ).toBeDisabled();
  });

  it("enables the thinking switch once the classifier queue is on", () => {
    classifierConfig = { enabled: true, forceThinkingOff: true };
    render(<ClassifierQueueManager appType="claude" />);

    expect(
      screen.getByRole("switch", { name: "proxy.classifier.forceThinkingOff" }),
    ).toBeEnabled();
  });

  it("renders queued providers in order with a position badge", () => {
    classifierQueue = [
      { providerId: "fast", providerName: "Fast Provider" },
      { providerId: "backup", providerName: "Backup Provider" },
    ];
    render(<ClassifierQueueManager appType="claude" />);

    expect(screen.getByText("Fast Provider")).toBeInTheDocument();
    expect(screen.getByText("Backup Provider")).toBeInTheDocument();
    expect(screen.getByText("1")).toBeInTheDocument();
    expect(screen.getByText("2")).toBeInTheDocument();
  });

  it("removes a provider from the queue", async () => {
    classifierQueue = [{ providerId: "fast", providerName: "Fast Provider" }];
    render(<ClassifierQueueManager appType="claude" />);

    fireEvent.click(screen.getByRole("button", { name: "common.delete" }));

    await waitFor(() =>
      expect(removeMock).toHaveBeenCalledWith({
        appType: "claude",
        providerId: "fast",
      }),
    );
  });
});
