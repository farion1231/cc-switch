import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { toast } from "sonner";
import { DeepLinkImportDialog } from "@/components/DeepLinkImportDialog";
import { emitTauriEvent } from "../msw/tauriMocks";

const apiMocks = vi.hoisted(() => ({
  preview: vi.fn(),
  confirm: vi.fn(),
  cancel: vi.fn(),
}));

vi.mock("@/lib/api/deeplink", async () => {
  const actual =
    await vi.importActual<typeof import("@/lib/api/deeplink")>(
      "@/lib/api/deeplink",
    );
  return {
    ...actual,
    deeplinkApi: {
      ...actual.deeplinkApi,
      previewProviderSwitch: apiMocks.preview,
      confirmProviderSwitch: apiMocks.confirm,
      cancelProviderSwitch: apiMocks.cancel,
    },
  };
});

vi.mock("@/components/ui/dialog", () => ({
  Dialog: ({ open, children }: any) =>
    open ? <div data-testid="dialog-root">{children}</div> : null,
  DialogContent: ({ children }: any) => <div>{children}</div>,
  DialogDescription: ({ children }: any) => <div>{children}</div>,
  DialogFooter: ({ children }: any) => <div>{children}</div>,
  DialogHeader: ({ children }: any) => <div>{children}</div>,
  DialogTitle: ({ children }: any) => <h2>{children}</h2>,
}));

vi.mock("sonner", () => ({
  toast: {
    error: vi.fn(),
    success: vi.fn(),
    warning: vi.fn(),
  },
}));

const request = {
  version: "v1",
  resource: "provider-switch",
  app: "codex",
  id: "target-provider",
};

function renderDialog() {
  return render(
    <QueryClientProvider client={new QueryClient()}>
      <DeepLinkImportDialog />
    </QueryClientProvider>,
  );
}

describe("provider-switch deep link confirmation", () => {
  beforeEach(() => {
    apiMocks.preview.mockReset();
    apiMocks.confirm.mockReset();
    apiMocks.cancel.mockReset();
    apiMocks.preview.mockResolvedValue({
      name: "Target Relay",
      hostname: "target.example",
      isCurrent: false,
      reviewToken: "opaque-review-token",
    });
    apiMocks.confirm.mockResolvedValue({
      name: "Target Relay",
      hostname: "target.example",
      isCurrent: true,
      hasWarnings: false,
    });
    apiMocks.cancel.mockResolvedValue(undefined);
  });

  it("previews the target but does not confirm when the owner cancels", async () => {
    renderDialog();

    await act(async () => {
      emitTauriEvent("deeplink-import", request);
    });

    await waitFor(() => expect(apiMocks.preview).toHaveBeenCalledWith(request));
    expect(await screen.findByText("Target Relay")).toBeInTheDocument();

    fireEvent.click(screen.getByText("common.cancel"));

    await waitFor(() =>
      expect(apiMocks.cancel).toHaveBeenCalledWith("opaque-review-token"),
    );
    expect(apiMocks.confirm).not.toHaveBeenCalled();
    await waitFor(() =>
      expect(screen.queryByTestId("dialog-root")).not.toBeInTheDocument(),
    );
  });

  it("keeps the review open until the cancellation barrier is released", async () => {
    let releaseCancel: (() => void) | undefined;
    apiMocks.cancel.mockImplementationOnce(
      () =>
        new Promise<void>((resolve) => {
          releaseCancel = resolve;
        }),
    );
    renderDialog();

    await act(async () => {
      emitTauriEvent("deeplink-import", request);
    });
    expect(await screen.findByText("Target Relay")).toBeInTheDocument();

    fireEvent.click(screen.getByText("common.cancel"));

    await waitFor(() => expect(apiMocks.cancel).toHaveBeenCalledTimes(1));
    expect(screen.getByTestId("dialog-root")).toBeInTheDocument();

    await act(async () => {
      releaseCancel?.();
    });
    await waitFor(() =>
      expect(screen.queryByTestId("dialog-root")).not.toBeInTheDocument(),
    );
  });

  it("confirms only after the owner clicks the switch button", async () => {
    renderDialog();

    await act(async () => {
      emitTauriEvent("deeplink-import", request);
    });
    expect(await screen.findByText("Target Relay")).toBeInTheDocument();

    fireEvent.click(screen.getByText("deeplink.providerSwitch.confirm"));

    await waitFor(() =>
      expect(apiMocks.confirm).toHaveBeenCalledWith("opaque-review-token"),
    );
    await waitFor(() =>
      expect(screen.queryByTestId("dialog-root")).not.toBeInTheDocument(),
    );
  });

  it("closes a consumed review after confirmation fails", async () => {
    apiMocks.confirm.mockRejectedValueOnce(new Error("review no longer valid"));
    renderDialog();

    await act(async () => {
      emitTauriEvent("deeplink-import", request);
    });
    expect(await screen.findByText("Target Relay")).toBeInTheDocument();

    fireEvent.click(screen.getByText("deeplink.providerSwitch.confirm"));

    await waitFor(() => expect(toast.error).toHaveBeenCalled());
    expect(screen.queryByTestId("dialog-root")).not.toBeInTheDocument();
    expect(apiMocks.confirm).toHaveBeenCalledTimes(1);
  });
});
