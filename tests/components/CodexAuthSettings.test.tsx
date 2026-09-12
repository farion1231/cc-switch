import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ComponentProps } from "react";
import { CodexAuthSettings } from "@/components/settings/CodexAuthSettings";

const mocks = vi.hoisted(() => ({
  migrate: vi.fn(),
  success: vi.fn(),
  error: vi.fn(),
  info: vi.fn(),
}));
vi.mock("@/lib/api", () => ({
  settingsApi: { migrateCodexUnifiedHistory: mocks.migrate },
}));
vi.mock("sonner", () => ({ toast: mocks }));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

function mount(migrateExisting: boolean) {
  const settings = {
    unifyCodexSessionHistory: true,
    unifyCodexMigrateExisting: migrateExisting,
  } as ComponentProps<typeof CodexAuthSettings>["settings"];
  return render(<CodexAuthSettings settings={settings} onChange={vi.fn()} />);
}

describe("Codex history migration retry", () => {
  beforeEach(() => vi.clearAllMocks());

  it("does not offer migration without existing-history consent", () => {
    mount(false);
    expect(
      screen.queryByText("settings.unifyCodexHistoryMigrationRetry"),
    ).toBeNull();
    expect(mocks.migrate).not.toHaveBeenCalled();
  });

  it("retries only on click and reports completed migration", async () => {
    mocks.migrate.mockResolvedValue({
      migratedJsonlFiles: 3,
      migratedStateRows: 1,
    });
    mount(true);
    expect(mocks.migrate).not.toHaveBeenCalled();
    fireEvent.click(
      screen.getByText("settings.unifyCodexHistoryMigrationRetry"),
    );
    await waitFor(() => expect(mocks.success).toHaveBeenCalled());
    expect(mocks.migrate).toHaveBeenCalledTimes(1);
  });

  it("shows writer errors without claiming success and permits retry", async () => {
    mocks.migrate.mockRejectedValue("Quit Codex before migrating");
    mount(true);
    const button = screen.getByText("settings.unifyCodexHistoryMigrationRetry");
    fireEvent.click(button);
    await waitFor(() =>
      expect(mocks.error).toHaveBeenCalledWith(
        "settings.unifyCodexHistoryMigrationFailed",
        { description: "Quit Codex before migrating" },
      ),
    );
    expect(mocks.success).not.toHaveBeenCalled();
    await waitFor(() => expect(button).not.toBeDisabled());
  });

  it("does not report skipped migrations as successful", async () => {
    mocks.migrate.mockResolvedValue({ skippedReason: "live_not_unified" });
    mount(true);
    fireEvent.click(
      screen.getByText("settings.unifyCodexHistoryMigrationRetry"),
    );
    await waitFor(() => expect(mocks.info).toHaveBeenCalled());
    expect(mocks.success).not.toHaveBeenCalled();
  });
});
