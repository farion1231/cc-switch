import { render, screen } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import "@testing-library/jest-dom";
import { AppVisibilitySettings } from "@/components/settings/AppVisibilitySettings";
import { DEFAULT_VISIBLE_APPS } from "@/config/appConfig";
import type { SettingsFormState } from "@/hooks/useSettings";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

vi.mock("@/components/ProviderIcon", () => ({
  ProviderIcon: () => null,
}));

describe("AppVisibilitySettings", () => {
  it("exposes a visibility toggle for every supported app", () => {
    render(
      <AppVisibilitySettings
        settings={
          { visibleApps: { ...DEFAULT_VISIBLE_APPS } } as SettingsFormState
        }
        onChange={vi.fn()}
      />,
    );

    for (const key of [
      "apps.claudeCode",
      "apps.claudeDesktop",
      "apps.codex",
      "apps.gemini",
      "apps.grokbuild",
      "apps.opencode",
      "apps.openclaw",
      "apps.hermes",
      "apps.pi",
      "apps.deepseek-harness",
    ]) {
      expect(screen.getByRole("button", { name: key })).toBeInTheDocument();
    }
  });
});
