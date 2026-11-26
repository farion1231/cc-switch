import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { beforeAll, describe, expect, it, vi } from "vitest";
import { AddProviderDialog } from "@/components/providers/AddProviderDialog";
import type { AppId, AuthCenterTarget } from "@/lib/api";
import { createTestQueryClient } from "../utils/testQueryClient";

vi.mock("@/lib/authBinding", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/authBinding")>();
  return {
    ...actual,
    canSwitchAgyAccount: () => true,
  };
});

vi.mock("@/components/settings/AuthCenterPanel", () => ({
  AuthCenterPanel: ({
    authScrollTarget,
  }: {
    authScrollTarget?: AuthCenterTarget | null;
  }) => <div data-testid="auth-center-mock">{authScrollTarget}</div>,
}));

vi.mock("@/components/JsonEditor", () => ({
  default: () => null,
}));

vi.mock("@/lib/api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/api")>();
  return {
    ...actual,
    providersApi: {
      ...actual.providersApi,
      getEditorView: vi.fn().mockResolvedValue({ settings: {}, inactive: [] }),
    },
    settingsApi: {
      ...actual.settingsApi,
      getAppMode: vi.fn().mockResolvedValue({ mode: "direct" }),
    },
  };
});

beforeAll(() => {
  Element.prototype.scrollIntoView ??= vi.fn();
  window.matchMedia ??= vi.fn().mockReturnValue({
    matches: false,
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
  });
});

describe("AddProviderDialog official presets route directly to Auth Center", () => {
  it.each<{
    appId: AppId;
    presetButton: string | RegExp;
    expectedTarget: AuthCenterTarget;
  }>([
    {
      appId: "claude",
      presetButton: /Claude Official/,
      expectedTarget: "claude_official",
    },
    {
      appId: "codex",
      presetButton: /OpenAI Official/,
      expectedTarget: "codex_oauth",
    },
    {
      appId: "grokbuild",
      presetButton: /Grok Official/,
      expectedTarget: "xai_oauth",
    },
    {
      appId: "agy",
      presetButton: /Google Official/,
      expectedTarget: "agy_oauth",
    },
    {
      appId: "copilot",
      presetButton: /GitHub Copilot/,
      expectedTarget: "github_copilot",
    },
    {
      appId: "workbuddy",
      presetButton: /workbuddy\.official\.presetName/,
      expectedTarget: "workbuddy_account",
    },
    {
      appId: "codebuddy",
      presetButton: /codebuddy\.official\.presetName/,
      expectedTarget: "codebuddy_oauth",
    },
    {
      appId: "kimi",
      presetButton: /kimi\.oauth\.title/,
      expectedTarget: "kimi_oauth",
    },
    {
      appId: "dsh",
      presetButton: /dsh\.account\.presetName/,
      expectedTarget: "dsh_account",
    },
  ])(
    "navigates directly to Auth Center ($expectedTarget) when clicking official preset on $appId",
    async ({ appId, presetButton, expectedTarget }) => {
      const onOpenChange = vi.fn();
      const onSubmit = vi.fn();
      const onOpenAuthCenter = vi.fn();
      render(
        <QueryClientProvider client={createTestQueryClient()}>
          <AddProviderDialog
            open
            appId={appId}
            onOpenChange={onOpenChange}
            onSubmit={onSubmit}
            onOpenAuthCenter={onOpenAuthCenter}
          />
        </QueryClientProvider>,
      );

      const button = await screen.findByRole("button", { name: presetButton });
      fireEvent.click(button);

      await waitFor(() => {
        expect(onOpenAuthCenter).toHaveBeenCalledWith(expectedTarget);
      });
      expect(onOpenChange).toHaveBeenCalledWith(false);
      expect(onSubmit).not.toHaveBeenCalled();
    },
  );

  it("opens the embedded AuthSettingsPanel directly when onOpenAuthCenter is omitted", async () => {
    const onOpenChange = vi.fn();
    render(
      <QueryClientProvider client={createTestQueryClient()}>
        <AddProviderDialog
          open
          appId="claude"
          onOpenChange={onOpenChange}
          onSubmit={vi.fn()}
        />
      </QueryClientProvider>,
    );

    const button = await screen.findByRole("button", {
      name: /Claude Official/,
    });
    fireEvent.click(button);

    expect(await screen.findByTestId("auth-center-mock")).toHaveTextContent(
      "claude_official",
    );
    expect(onOpenChange).not.toHaveBeenCalled();
  });
});
