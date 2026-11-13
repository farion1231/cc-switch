import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ClineHubNotice } from "@/components/providers/ClineHubNotice";
import type { ClineHubStatus } from "@/lib/api/cline";

const mocks = vi.hoisted(() => ({
  getHubStatus: vi.fn<() => Promise<ClineHubStatus>>(),
  copyText: vi.fn<(text: string) => Promise<void>>(),
  success: vi.fn(),
  error: vi.fn(),
}));

vi.mock("@/lib/api/cline", () => ({
  clineApi: { getHubStatus: mocks.getHubStatus },
}));
vi.mock("@/lib/clipboard", () => ({ copyText: mocks.copyText }));
vi.mock("@/lib/toast", () => ({
  toast: { success: mocks.success, error: mocks.error },
}));

function renderNotice() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <ClineHubNotice />
    </QueryClientProvider>,
  );
}

describe("ClineHubNotice", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.copyText.mockResolvedValue();
  });

  it("warns about a stale hub and copies the restart command", async () => {
    mocks.getHubStatus.mockResolvedValue({
      running: true,
      stale: true,
      pid: 4242,
      startedAt: "2026-10-23T02:12:41.196Z",
    });
    renderNotice();
    expect(await screen.findByText("cline.hub.staleTitle")).toBeInTheDocument();
    expect(screen.getByText("cline.hub.staleBody")).toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("button", { name: "cline.hub.copyCommand" }),
    );
    await waitFor(() =>
      expect(mocks.copyText).toHaveBeenCalledWith("cline doctor fix"),
    );
    await waitFor(() => expect(mocks.success).toHaveBeenCalled());
  });

  it.each([
    { running: true, stale: false },
    { running: false, stale: false },
  ])(
    "stays hidden when the hub is up to date or not running (%o)",
    async (status) => {
      mocks.getHubStatus.mockResolvedValue(status);
      const { container } = renderNotice();
      await waitFor(() => expect(mocks.getHubStatus).toHaveBeenCalled());
      expect(container).toBeEmptyDOMElement();
    },
  );
});
