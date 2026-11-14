import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { WhatsNewEntry } from "@/lib/whatsNew";

const mocks = vi.hoisted(() => ({
  getAll: vi.fn(),
  save: vi.fn(),
  openExternal: vi.fn(),
  version: "4.0.4",
  settings: undefined as
    | {
        firstRunNoticeConfirmed?: boolean;
        newLayoutNoticeConfirmed?: boolean;
        whatsNewSeenVersion?: string;
      }
    | undefined,
  entries: [] as WhatsNewEntry[],
}));

vi.mock("@/lib/api", () => ({
  providersApi: { getAll: mocks.getAll },
  settingsApi: { save: mocks.save, openExternal: mocks.openExternal },
}));
vi.mock("@/lib/query", () => ({
  useSettingsQuery: () => ({ data: mocks.settings }),
}));
vi.mock("@tauri-apps/api/app", () => ({
  getVersion: async () => mocks.version,
}));
vi.mock("@/lib/whatsNew", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/whatsNew")>()),
  get WHATS_NEW_ENTRIES() {
    return mocks.entries;
  },
}));

import { WhatsNewNotice } from "@/components/WhatsNewDialog";

const entry = (version: string, ...texts: string[]): WhatsNewEntry => ({
  version,
  items: texts.map((text) => ({
    type: "fix",
    zh: text,
    "zh-TW": text,
    en: `${text}-en`,
    ja: `${text}-ja`,
  })),
});

function renderNotice() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <WhatsNewNotice />
    </QueryClientProvider>,
  );
}

const title = () => screen.queryByText("whatsNew.updatedTo");

/** 「不该弹」的用例：等异步查询都落定后再断言 */
const settle = () =>
  act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 20));
  });

describe("WhatsNewNotice", () => {
  beforeEach(() => {
    mocks.version = "4.0.4";
    mocks.settings = {
      firstRunNoticeConfirmed: true,
      newLayoutNoticeConfirmed: true,
      whatsNewSeenVersion: "4.0.1",
    };
    mocks.entries = [
      entry("4.1.0", "未来版本"),
      entry("4.0.4", "第四版甲", "第四版乙"),
      entry("4.0.3"),
      entry("4.0.2", "第二版"),
      entry("4.0.1", "第一版"),
    ];
    mocks.save.mockReset().mockResolvedValue(true);
    mocks.openExternal.mockReset().mockResolvedValue(undefined);
    mocks.getAll.mockReset().mockResolvedValue({ p1: { id: "p1" } });
  });

  it("lists every version since the last one seen and records the current version", async () => {
    renderNotice();
    expect(await screen.findByText("whatsNew.updatedTo")).toBeInTheDocument();
    expect(screen.getByText("whatsNew.since")).toBeInTheDocument();
    expect(screen.getByText("v4.0.4")).toBeInTheDocument();
    expect(screen.getByText("第四版乙")).toBeInTheDocument();
    expect(screen.getByText("v4.0.2")).toBeInTheDocument();
    expect(screen.queryByText("v4.0.1")).not.toBeInTheDocument();
    expect(screen.queryByText("v4.0.3")).not.toBeInTheDocument();
    expect(screen.queryByText("v4.1.0")).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "whatsNew.confirm" }));
    await waitFor(() => expect(title()).not.toBeInTheDocument());
    expect(mocks.save).toHaveBeenCalledWith(
      expect.objectContaining({ whatsNewSeenVersion: "4.0.4" }),
    );
  });

  it("shows only the current version when nothing was recorded before", async () => {
    mocks.settings = {
      firstRunNoticeConfirmed: true,
      newLayoutNoticeConfirmed: true,
    };
    renderNotice();
    expect(await screen.findByText("v4.0.4")).toBeInTheDocument();
    expect(screen.queryByText("v4.0.2")).not.toBeInTheDocument();
    expect(screen.queryByText("whatsNew.since")).not.toBeInTheDocument();
  });

  it("stays closed when the current version was already seen", async () => {
    mocks.settings!.whatsNewSeenVersion = "4.0.4";
    renderNotice();
    await settle();
    expect(title()).not.toBeInTheDocument();
    expect(mocks.save).not.toHaveBeenCalled();
  });

  it("stays closed after a downgrade and keeps the higher version", async () => {
    mocks.version = "4.0.2";
    mocks.settings!.whatsNewSeenVersion = "4.0.4";
    renderNotice();
    await settle();
    expect(title()).not.toBeInTheDocument();
    expect(mocks.save).not.toHaveBeenCalled();
  });

  it("silently records the version when there is nothing to show", async () => {
    mocks.version = "4.0.3";
    mocks.settings!.whatsNewSeenVersion = "4.0.2";
    renderNotice();
    await waitFor(() =>
      expect(mocks.save).toHaveBeenCalledWith(
        expect.objectContaining({ whatsNewSeenVersion: "4.0.3" }),
      ),
    );
    expect(title()).not.toBeInTheDocument();
  });

  it("waits for the welcome dialog on a fresh install", async () => {
    mocks.settings = { firstRunNoticeConfirmed: undefined };
    renderNotice();
    await settle();
    expect(title()).not.toBeInTheDocument();
    expect(mocks.save).not.toHaveBeenCalled();
  });

  it("waits while the new layout dialog is showing", async () => {
    mocks.settings = {
      firstRunNoticeConfirmed: true,
      whatsNewSeenVersion: "4.0.1",
    };
    renderNotice();
    await waitFor(() => expect(mocks.getAll).toHaveBeenCalled());
    await settle();
    expect(title()).not.toBeInTheDocument();
    expect(mocks.save).not.toHaveBeenCalled();
  });

  it("shows when the new layout dialog does not apply", async () => {
    mocks.settings = {
      firstRunNoticeConfirmed: true,
      whatsNewSeenVersion: "4.0.1",
    };
    mocks.getAll.mockResolvedValue({});
    renderNotice();
    expect(await screen.findByText("whatsNew.updatedTo")).toBeInTheDocument();
  });

  it("collapses older versions and opens the changelog list", async () => {
    mocks.settings!.whatsNewSeenVersion = "4.0.0";
    mocks.entries = [
      entry("4.0.4", "第四版"),
      entry("4.0.3", "第三版"),
      entry("4.0.2", "第二版"),
      entry("4.0.1", "第一版"),
    ];
    renderNotice();
    await screen.findByText("v4.0.4");
    expect(screen.queryByText("v4.0.1")).not.toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("button", { name: "whatsNew.showEarlier" }),
    );
    expect(screen.getByText("v4.0.1")).toBeInTheDocument();

    fireEvent.click(
      screen.getAllByRole("button", { name: "whatsNew.versionDetails" })[0],
    );
    expect(mocks.openExternal).toHaveBeenCalledWith(
      "https://ccswitch.io/zh/changelog/4.0.4",
    );

    fireEvent.click(screen.getByRole("button", { name: "whatsNew.viewFull" }));
    expect(mocks.openExternal).toHaveBeenLastCalledWith(
      "https://ccswitch.io/zh/changelog",
    );
    await waitFor(() => expect(title()).not.toBeInTheDocument());
    expect(mocks.save).toHaveBeenCalledWith(
      expect.objectContaining({ whatsNewSeenVersion: "4.0.4" }),
    );
  });
});
