import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it } from "vitest";
import { http, HttpResponse } from "msw";
import type { ReactElement } from "react";
import { server } from "../msw/server";
import { ClaudeDesktopSettings } from "@/components/settings/ClaudeDesktopSettings";

const TAURI_ENDPOINT = "http://tauri.local";

function renderWithQueryClient(ui: ReactElement) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });

  return render(
    <QueryClientProvider client={queryClient}>{ui}</QueryClientProvider>,
  );
}

function claudeDesktopStatus(disableAutoUpdates: boolean) {
  return {
    supported: true,
    configured: true,
    appliedId: "00000000-0000-4000-8000-000000157210",
    profilePath: null,
    configLibraryPath: null,
    mode: "direct",
    expectedBaseUrl: null,
    actualBaseUrl: null,
    proxyRunning: false,
    staleRawModels: false,
    missingRouteMappings: false,
    gatewayTokenConfigured: true,
    disableAutoUpdates,
  };
}

describe("ClaudeDesktopSettings", () => {
  it("renders in settings and writes the toggled auto-update setting", async () => {
    const writes: boolean[] = [];

    server.use(
      http.post(`${TAURI_ENDPOINT}/get_claude_desktop_status`, () =>
        HttpResponse.json(claudeDesktopStatus(true)),
      ),
      http.post(
        `${TAURI_ENDPOINT}/set_claude_desktop_disable_auto_updates`,
        async ({ request }) => {
          const body = (await request.json()) as { enabled: boolean };
          writes.push(body.enabled);
          return HttpResponse.json(true);
        },
      ),
    );

    renderWithQueryClient(<ClaudeDesktopSettings />);

    expect(await screen.findByText("Claude Desktop 设置")).toBeInTheDocument();

    const toggle = await screen.findByRole("switch", {
      name: "关闭 Claude Desktop 自动更新",
    });
    await waitFor(() => {
      expect(toggle).toBeChecked();
    });

    fireEvent.click(toggle);

    await waitFor(() => {
      expect(writes).toEqual([false]);
    });
  });
});
