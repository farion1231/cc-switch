import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { WhatsNewEntry } from "@/lib/whatsNew";

const mocks = vi.hoisted(() => ({
  availableVersion: "4.0.14" as string | undefined,
  notes: undefined as string | undefined,
}));

vi.mock("@tauri-apps/api/app", () => ({
  getVersion: async () => "4.0.13",
}));
vi.mock("@/contexts/UpdateContext", () => ({
  useUpdate: () => ({
    hasUpdate: Boolean(mocks.availableVersion),
    updateInfo: mocks.availableVersion
      ? { availableVersion: mocks.availableVersion, notes: mocks.notes }
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
describe("AboutSection recent updates", () => {
  beforeEach(() => {
    localStorage.clear();
    mocks.availableVersion = "4.0.14";
    mocks.notes = JSON.stringify(pendingEntry);
  });

  it("marks the pending summary from notes without another request", async () => {
    const requests = vi.spyOn(globalThis, "fetch");
    render(<AboutSection isPortable={false} />);
    expect(screen.queryByText(mocks.notes!)).not.toBeInTheDocument();
    fireEvent.click(
      await screen.findByRole("button", { name: "whatsNew.recentTitle" }),
    );

    const dialog = await screen.findByRole("dialog");
    const badge = await within(dialog).findByText("whatsNew.pendingUpdate");
    expect(badge.closest("h3")).toHaveTextContent("v4.0.14");
    expect(within(dialog).getAllByText("whatsNew.pendingUpdate")).toHaveLength(
      1,
    );
    expect(within(dialog).getByText("待安装版本的修复")).toBeInTheDocument();
    expect(
      within(dialog).getByRole("heading", { name: "v4.0.13" }),
    ).toBeInTheDocument();
    expect(requests).not.toHaveBeenCalled();
    requests.mockRestore();
  });

  it.each([
    undefined,
    "Release v4.0.14",
    '{"version":',
    JSON.stringify({ ...pendingEntry, version: "4.0.15" }),
    JSON.stringify({ version: "4.0.14", items: [] }),
  ])(
    "keeps local summaries when notes have no usable summary: %s",
    async (notes) => {
      mocks.notes = notes;
      render(<AboutSection isPortable={false} />);
      fireEvent.click(
        await screen.findByRole("button", { name: "whatsNew.recentTitle" }),
      );

      const dialog = await screen.findByRole("dialog");
      expect(
        within(dialog).getByRole("heading", { name: "v4.0.13" }),
      ).toBeInTheDocument();
      expect(
        within(dialog).queryByText("whatsNew.pendingUpdate"),
      ).not.toBeInTheDocument();
    },
  );

  it("updates the pending summary when the check result changes", async () => {
    const { rerender } = render(<AboutSection isPortable={false} />);
    fireEvent.click(
      await screen.findByRole("button", { name: "whatsNew.recentTitle" }),
    );
    const dialog = await screen.findByRole("dialog");
    expect(
      await within(dialog).findByText("待安装版本的修复"),
    ).toBeInTheDocument();

    mocks.notes = "Release v4.0.14";
    rerender(<AboutSection isPortable={false} />);
    expect(
      within(dialog).queryByText("待安装版本的修复"),
    ).not.toBeInTheDocument();

    mocks.notes = JSON.stringify(pendingEntry);
    rerender(<AboutSection isPortable={false} />);
    expect(within(dialog).getByText("待安装版本的修复")).toBeInTheDocument();

    mocks.availableVersion = undefined;
    rerender(<AboutSection isPortable={false} />);
    await waitFor(() =>
      expect(
        within(dialog).queryByText("whatsNew.pendingUpdate"),
      ).not.toBeInTheDocument(),
    );
  });
});
