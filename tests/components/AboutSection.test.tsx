import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { http, HttpResponse } from "msw";
import { server } from "../msw/server";
import type { WhatsNewEntry } from "@/lib/whatsNew";

const mocks = vi.hoisted(() => ({
  availableVersion: "4.0.14" as string | undefined,
}));

vi.mock("@tauri-apps/api/app", () => ({
  getVersion: async () => "4.0.13",
}));
vi.mock("@/contexts/UpdateContext", () => ({
  useUpdate: () => ({
    hasUpdate: Boolean(mocks.availableVersion),
    updateInfo: mocks.availableVersion
      ? { availableVersion: mocks.availableVersion }
      : null,
    checkUpdate: vi.fn(),
    resetDismiss: vi.fn(),
    isChecking: false,
  }),
}));

import { AboutSection } from "@/components/settings/AboutSection";

const pendingEntry: WhatsNewEntry = {
  version: "4.0.14",
  items: [
    {
      type: "fix",
      zh: "待安装版本的修复",
      "zh-TW": "待安裝版本的修復",
      en: "Fix in the pending update",
      ja: "未インストールの更新の修正",
    },
  ],
};
const cacheKey = "ccswitch:whats-new:remote:4.0.14";

describe("AboutSection recent updates", () => {
  beforeEach(() => {
    localStorage.clear();
    mocks.availableVersion = "4.0.14";
  });

  it.each([false, true])(
    "marks the pending version in the real dialog (cached: %s)",
    async (cached) => {
      if (cached) localStorage.setItem(cacheKey, JSON.stringify(pendingEntry));
      const requests = vi.fn();
      server.use(
        http.post(
          "http://tauri.local/fetch_whats_new_summary",
          async ({ request }) => {
            requests(await request.json());
            return HttpResponse.json(pendingEntry);
          },
        ),
      );

      render(<AboutSection isPortable={false} />);
      await waitFor(() =>
        expect(localStorage.getItem(cacheKey)).not.toBeNull(),
      );
      fireEvent.click(
        await screen.findByRole("button", { name: "whatsNew.recentTitle" }),
      );

      const dialog = await screen.findByRole("dialog");
      const badge = await within(dialog).findByText("whatsNew.pendingUpdate");
      expect(badge.closest("h3")).toHaveTextContent("v4.0.14");
      expect(
        within(dialog).getAllByText("whatsNew.pendingUpdate"),
      ).toHaveLength(1);
      expect(within(dialog).getByText("待安装版本的修复")).toBeInTheDocument();
      expect(
        within(dialog).getByRole("heading", { name: "v4.0.13" }),
      ).toBeInTheDocument();
      if (cached) {
        expect(requests).not.toHaveBeenCalled();
      } else {
        expect(requests).toHaveBeenCalledTimes(1);
        expect(requests).toHaveBeenCalledWith({
          version: "4.0.14",
          source: "mirror",
        });
      }
    },
  );

  it("keeps installed summaries available when both remote sources fail", async () => {
    const requests = vi.fn();
    server.use(
      http.post(
        "http://tauri.local/fetch_whats_new_summary",
        async ({ request }) => {
          requests(await request.json());
          return new HttpResponse("offline", { status: 503 });
        },
      ),
    );

    render(<AboutSection isPortable={false} />);
    await waitFor(() => expect(requests).toHaveBeenCalledTimes(2));
    fireEvent.click(
      screen.getByRole("button", { name: "whatsNew.recentTitle" }),
    );

    const dialog = await screen.findByRole("dialog");
    expect(
      within(dialog).getByRole("heading", { name: "v4.0.13" }),
    ).toBeInTheDocument();
    expect(
      within(dialog).queryByText("whatsNew.pendingUpdate"),
    ).not.toBeInTheDocument();
    expect(localStorage.getItem(cacheKey)).toBeNull();
  });
});
