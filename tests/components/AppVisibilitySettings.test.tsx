import {fireEvent, render, screen, waitFor} from "@testing-library/react";
import {QueryClientProvider} from "@tanstack/react-query";
import {describe, expect, it, vi, beforeEach} from "vitest";
import {AppVisibilitySettings} from "@/components/settings/AppVisibilitySettings";
import {createTestQueryClient} from "../utils/testQueryClient";
import type {SettingsFormState} from "@/hooks/useSettings";
import {http} from "msw";
import {server} from "../msw/server";

const TAURI_ENDPOINT = "http://tauri.local";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({t: (key: string) => key}),
}));

const baseSettings: SettingsFormState = {
  showInTray: true,
  minimizeToTrayOnClose: false,
  visibleApps: {
    claude: true,
    "claude-desktop": true,
    codex: true,
    gemini: true,
    grokbuild: true,
    opencode: true,
    openclaw: true,
    hermes: false,
    pi: true,
    ohmypi: false,
  },
  showProfileSwitcher: true,
  language: "en",
};

function renderSettings(onChange: (updates: Partial<SettingsFormState>) => void) {
  const queryClient = createTestQueryClient();
  return render(
    <QueryClientProvider client={queryClient}>
      <AppVisibilitySettings settings={baseSettings} onChange={onChange} />
    </QueryClientProvider>,
  );
}

const ohmypiButton = () =>
  screen.getByRole("button", {name: /apps\.ohmypi/});

const confirmButton = () =>
  screen.getByRole("button", {
    name: "ohmypi.autoDiscovery.dialog.confirmBtn",
  });

const cancelButton = () =>
  screen.getByRole("button", {name: "common.cancel"});

describe("AppVisibilitySettings — ohmypi auto-discovery guard", () => {
  beforeEach(() => {
    server.resetHandlers();
  });

  it("opens confirm dialog on ohmypi off→on when needsConfirmation", async () => {
    const onChange = vi.fn();
    renderSettings(onChange);

    fireEvent.click(ohmypiButton());

    // Dialog should appear with confirm/cancel buttons
    await waitFor(() => {
      expect(confirmButton()).toBeInTheDocument();
    });
    expect(cancelButton()).toBeInTheDocument();
    // ohmypi not enabled yet
    expect(onChange).not.toHaveBeenCalled();
  });

  it("enables ohmypi after confirming the dialog", async () => {
    const onChange = vi.fn();
    renderSettings(onChange);

    fireEvent.click(ohmypiButton());
    await waitFor(() => expect(confirmButton()).toBeInTheDocument());

    fireEvent.click(confirmButton());

    await waitFor(() => {
      expect(onChange).toHaveBeenCalledWith(
        expect.objectContaining({
          visibleApps: expect.objectContaining({ohmypi: true}),
        }),
      );
    });
  });

  it("does not enable ohmypi when dialog is cancelled", async () => {
    const onChange = vi.fn();
    renderSettings(onChange);

    fireEvent.click(ohmypiButton());
    await waitFor(() => expect(cancelButton()).toBeInTheDocument());

    fireEvent.click(cancelButton());

    // Give a tick for any async work
    const {promise, resolve} = (() => {
      let r: () => void;
      const p = new Promise<void>((res) => (r = res));
      return {promise: p, resolve: r!};
    })();
    setTimeout(resolve, 50);
    await promise;
    expect(onChange).not.toHaveBeenCalled();
  });

  it("directly enables ohmypi without dialog when needsConfirmation is false", async () => {
    server.use(
      http.post(`${TAURI_ENDPOINT}/get_ohmypi_agent_discovery_state`, () =>
        Response.json({
          needsConfirmation: false,
          requiredProviderIds: [],
          missingProviderIds: [],
        }),
      ),
    );

    const onChange = vi.fn();
    renderSettings(onChange);

    fireEvent.click(ohmypiButton());

    await waitFor(() => {
      expect(onChange).toHaveBeenCalledWith(
        expect.objectContaining({
          visibleApps: expect.objectContaining({ohmypi: true}),
        }),
      );
    });

    // Dialog should not appear
    expect(
      screen.queryByRole("button", {
        name: "ohmypi.autoDiscovery.dialog.confirmBtn",
      }),
    ).not.toBeInTheDocument();
  });
});
