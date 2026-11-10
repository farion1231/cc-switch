import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { http, HttpResponse } from "msw";
import type { Settings } from "@/types";
import { CodexPauseOfficialLoginSection } from "@/components/providers/mode/CodexPauseOfficialLogin";
import { server } from "../msw/server";
import { getSettings, setSettings } from "../msw/state";
import { createTestQueryClient } from "../utils/testQueryClient";

const { toastSuccess, toastError } = vi.hoisted(() => ({
  toastSuccess: vi.fn(),
  toastError: vi.fn(),
}));
vi.mock("@/lib/toast", () => ({
  toast: { success: toastSuccess, error: toastError },
}));

const TAURI_ENDPOINT = "http://tauri.local";
const key = (name: string) => `settings.codexPauseOfficialLogin.${name}`;

function renderSection(settings: Partial<Settings>) {
  setSettings(settings);
  return render(
    <QueryClientProvider client={createTestQueryClient()}>
      <CodexPauseOfficialLoginSection />
    </QueryClientProvider>,
  );
}

const toggle = () => screen.getByRole("switch", { name: key("title") });

describe("CodexPauseOfficialLoginSection", () => {
  beforeEach(() => {
    toastSuccess.mockClear();
    toastError.mockClear();
  });

  it("is off by default and saves the pause with a restart hint", async () => {
    renderSection({ codexProxyWithoutOfficialLogin: undefined });
    await waitFor(() => expect(toggle()).toBeEnabled());
    expect(toggle()).not.toBeChecked();

    await userEvent.click(toggle());

    await waitFor(() => expect(toggle()).toBeChecked());
    expect(getSettings().codexProxyWithoutOfficialLogin).toBe(true);
    expect(toastSuccess).toHaveBeenLastCalledWith(key("pausedToast"));
  });

  it("resumes the official login with its own hint", async () => {
    renderSection({ codexProxyWithoutOfficialLogin: true });
    await waitFor(() => expect(toggle()).toBeChecked());

    await userEvent.click(toggle());

    await waitFor(() => expect(toggle()).not.toBeChecked());
    expect(getSettings().codexProxyWithoutOfficialLogin).toBe(false);
    expect(toastSuccess).toHaveBeenLastCalledWith(key("resumedToast"));
  });

  it("stays off and reports the error when the backend refuses the change", async () => {
    renderSection({ codexProxyWithoutOfficialLogin: false });
    server.use(
      http.post(`${TAURI_ENDPOINT}/save_settings`, () =>
        HttpResponse.json("Codex 配置重写失败", { status: 500 }),
      ),
    );
    await waitFor(() => expect(toggle()).toBeEnabled());

    await userEvent.click(toggle());

    await waitFor(() =>
      expect(toastError).toHaveBeenCalledWith(key("saveFailed")),
    );
    expect(toggle()).not.toBeChecked();
    expect(toastSuccess).not.toHaveBeenCalled();
  });
});
