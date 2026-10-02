import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  getAll: vi.fn(),
  settings: undefined as { firstRunNoticeConfirmed?: boolean } | undefined,
}));

vi.mock("@/lib/api", () => ({
  providersApi: { getAll: mocks.getAll },
}));
vi.mock("@/lib/query", () => ({
  useSettingsQuery: () => ({ data: mocks.settings }),
}));

import {
  NEW_LAYOUT_NOTICE_KEY,
  NewLayoutNotice,
} from "@/components/shell/NewLayoutNotice";

function renderNotice() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <NewLayoutNotice />
    </QueryClientProvider>,
  );
}

describe("NewLayoutNotice", () => {
  beforeEach(() => {
    window.localStorage.clear();
    mocks.settings = { firstRunNoticeConfirmed: true };
    mocks.getAll
      .mockReset()
      .mockImplementation(async (app: string) =>
        app === "codex" ? { p1: { id: "p1" } } : {},
      );
  });

  it("shows once for returning users and stays dismissed", async () => {
    const view = renderNotice();
    expect(await screen.findByText("coach.newLayout")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "common.close" }));
    expect(screen.queryByText("coach.newLayout")).not.toBeInTheDocument();
    expect(window.localStorage.getItem(NEW_LAYOUT_NOTICE_KEY)).toBe("1");

    view.unmount();
    renderNotice();
    await waitFor(() => expect(mocks.getAll).toHaveBeenCalled());
    expect(screen.queryByText("coach.newLayout")).not.toBeInTheDocument();
  });

  it("stays hidden when no app has providers", async () => {
    mocks.getAll.mockResolvedValue({});
    renderNotice();
    await waitFor(() => expect(mocks.getAll).toHaveBeenCalledTimes(10));
    expect(screen.queryByText("coach.newLayout")).not.toBeInTheDocument();
  });

  it("never shows to a fresh install that is seeing the welcome dialog", async () => {
    mocks.settings = { firstRunNoticeConfirmed: undefined };
    const view = renderNotice();
    await waitFor(() =>
      expect(window.localStorage.getItem(NEW_LAYOUT_NOTICE_KEY)).toBe("1"),
    );
    expect(mocks.getAll).not.toHaveBeenCalled();

    // 欢迎弹窗确认之后字段变成 true，也不再补弹「改版」
    mocks.settings = { firstRunNoticeConfirmed: true };
    view.rerender(
      <QueryClientProvider client={new QueryClient()}>
        <NewLayoutNotice />
      </QueryClientProvider>,
    );
    expect(screen.queryByText("coach.newLayout")).not.toBeInTheDocument();
  });

  it("still works when localStorage throws", async () => {
    const getItem = vi
      .spyOn(Storage.prototype, "getItem")
      .mockImplementation(() => {
        throw new Error("blocked");
      });
    const setItem = vi
      .spyOn(Storage.prototype, "setItem")
      .mockImplementation(() => {
        throw new Error("blocked");
      });
    try {
      renderNotice();
      expect(await screen.findByText("coach.newLayout")).toBeInTheDocument();
      fireEvent.click(screen.getByRole("button", { name: "common.close" }));
      expect(screen.queryByText("coach.newLayout")).not.toBeInTheDocument();
    } finally {
      getItem.mockRestore();
      setItem.mockRestore();
    }
  });
});
