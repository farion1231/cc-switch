import type { ReactNode } from "react";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { http, HttpResponse } from "msw";
import { describe, expect, it } from "vitest";
import { UnreadableEntriesNotice } from "@/components/common/UnreadableEntriesNotice";
import { providersApi } from "@/lib/api/providers";
import type { UnreadableEntry } from "@/types";
import { server } from "../msw/server";
import { createTestQueryClient } from "../utils/testQueryClient";

const broken: UnreadableEntry = {
  id: "broken",
  name: "Broken Relay",
  error: "供应商 broken 的 settings_config 无法解析，已保持原样未改动: EOF",
};

function renderProviderNotice() {
  const client = createTestQueryClient();
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={client}>{children}</QueryClientProvider>
  );
  return render(
    <UnreadableEntriesNotice
      queryKey={["providers", "claude", "unreadable"]}
      load={() => providersApi.getUnreadable("claude")}
      remove={(id) => providersApi.delete(id, "claude")}
      invalidate={[["providers", "claude"]]}
    />,
    { wrapper },
  );
}

describe("UnreadableEntriesNotice", () => {
  it("lists an unreadable provider with its error and offers only deletion", async () => {
    let entries = [broken];
    const deleted: unknown[] = [];
    server.use(
      http.post("http://tauri.local/get_unreadable_providers", () =>
        HttpResponse.json(entries),
      ),
      http.post("http://tauri.local/delete_provider", async ({ request }) => {
        deleted.push(await request.json());
        entries = [];
        return HttpResponse.json(true);
      }),
    );
    renderProviderNotice();

    expect(
      await screen.findByText("「Broken Relay」的数据无法读取"),
    ).toBeInTheDocument();
    expect(
      screen.getByText(broken.error, { exact: false }),
    ).toBeInTheDocument();
    expect(screen.getAllByRole("button").map((b) => b.textContent)).toEqual([
      "common.delete",
    ]);

    fireEvent.click(screen.getByRole("button", { name: "common.delete" }));
    const dialog = await screen.findByRole("dialog");
    fireEvent.click(
      Array.from(dialog.querySelectorAll("button")).find(
        (button) => button.textContent === "common.delete",
      )!,
    );

    await waitFor(() =>
      expect(
        screen.queryByText("「Broken Relay」的数据无法读取"),
      ).not.toBeInTheDocument(),
    );
    expect(deleted).toEqual([{ id: "broken", app: "claude" }]);
  });

  it("renders nothing when every entry is readable", async () => {
    const { container } = renderProviderNotice();
    await waitFor(() => expect(container).toBeEmptyDOMElement());
  });
});
