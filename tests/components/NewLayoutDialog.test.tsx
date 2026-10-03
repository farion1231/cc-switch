import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  getAll: vi.fn(),
  save: vi.fn(),
  openExternal: vi.fn(),
  settings: undefined as
    | { firstRunNoticeConfirmed?: boolean; newLayoutNoticeConfirmed?: boolean }
    | undefined,
}));

vi.mock("@/lib/api", () => ({
  providersApi: { getAll: mocks.getAll },
  settingsApi: { save: mocks.save, openExternal: mocks.openExternal },
}));
vi.mock("@/lib/query", () => ({
  useSettingsQuery: () => ({ data: mocks.settings }),
}));
vi.mock("@tauri-apps/api/app", () => ({
  getVersion: async () => "4.0.0",
}));

import { NewLayoutDialog } from "@/components/shell/NewLayoutDialog";

function renderDialog() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <NewLayoutDialog />
    </QueryClientProvider>,
  );
}

describe("NewLayoutDialog", () => {
  beforeEach(() => {
    mocks.settings = { firstRunNoticeConfirmed: true };
    mocks.save.mockReset().mockResolvedValue(true);
    mocks.openExternal.mockReset().mockResolvedValue(undefined);
    mocks.getAll
      .mockReset()
      .mockImplementation(async (app: string) =>
        app === "codex" ? { p1: { id: "p1" } } : {},
      );
  });

  it("shows to returning users and records the confirmation", async () => {
    renderDialog();
    expect(
      await screen.findByText("newLayoutNotice.title"),
    ).toBeInTheDocument();

    fireEvent.click(
      screen.getByRole("button", { name: "newLayoutNotice.confirm" }),
    );
    await waitFor(() =>
      expect(
        screen.queryByText("newLayoutNotice.title"),
      ).not.toBeInTheDocument(),
    );
    expect(mocks.save).toHaveBeenCalledWith(
      expect.objectContaining({ newLayoutNoticeConfirmed: true }),
    );
  });

  it("opens this version's release notes and closes", async () => {
    renderDialog();
    fireEvent.click(
      await screen.findByRole("button", {
        name: "newLayoutNotice.viewReleaseNotes",
      }),
    );
    await waitFor(() =>
      expect(mocks.openExternal).toHaveBeenCalledWith(
        "https://github.com/farion1231/cc-switch/releases/tag/v4.0.0",
      ),
    );
    await waitFor(() =>
      expect(mocks.save).toHaveBeenCalledWith(
        expect.objectContaining({ newLayoutNoticeConfirmed: true }),
      ),
    );
    expect(screen.queryByText("newLayoutNotice.title")).not.toBeInTheDocument();
  });

  it("stays closed once confirmed", async () => {
    mocks.settings = {
      firstRunNoticeConfirmed: true,
      newLayoutNoticeConfirmed: true,
    };
    renderDialog();
    expect(mocks.getAll).not.toHaveBeenCalled();
    expect(screen.queryByText("newLayoutNotice.title")).not.toBeInTheDocument();
  });

  it("stays closed when no app has providers", async () => {
    mocks.getAll.mockResolvedValue({});
    renderDialog();
    await waitFor(() => expect(mocks.getAll).toHaveBeenCalledTimes(10));
    expect(screen.queryByText("newLayoutNotice.title")).not.toBeInTheDocument();
  });

  it("never shows to a fresh install that is seeing the welcome dialog", async () => {
    mocks.settings = { firstRunNoticeConfirmed: undefined };
    renderDialog();
    expect(mocks.getAll).not.toHaveBeenCalled();
    expect(screen.queryByText("newLayoutNotice.title")).not.toBeInTheDocument();
  });
});
