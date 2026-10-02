import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { SkillsStorageSheet } from "@/components/skills/SkillsStorageSheet";

const m = vi.hoisted(() => ({
  resync: vi.fn(),
  toastSuccess: vi.fn(),
  toastWarning: vi.fn(),
  toastError: vi.fn(),
}));

vi.mock("sonner", () => ({
  toast: {
    success: m.toastSuccess,
    warning: m.toastWarning,
    error: m.toastError,
  },
}));

vi.mock("@/hooks/useSettings", () => ({
  useSettings: () => ({
    settings: { skillStorageLocation: "cc_switch", skillSyncMethod: "auto" },
    updateSettings: vi.fn(),
    autoSaveSettings: vi.fn().mockResolvedValue(undefined),
  }),
}));

vi.mock("@/hooks/useSkills", () => ({
  useInstalledSkills: () => ({ data: [] }),
  useResyncSkillsToApps: () => ({ mutateAsync: m.resync, isPending: false }),
}));

vi.mock("@/components/settings/SkillStorageLocationSettings", () => ({
  SkillStorageLocationSettings: () => <div data-testid="location" />,
}));

vi.mock("@/components/settings/SkillSyncMethodSettings", () => ({
  SkillSyncMethodSettings: () => <div data-testid="sync-method" />,
}));

const renderSheet = () =>
  render(<SkillsStorageSheet open onOpenChange={vi.fn()} />);

describe("SkillsStorageSheet", () => {
  beforeEach(() => {
    m.resync.mockReset();
    m.toastSuccess.mockReset();
    m.toastWarning.mockReset();
    m.toastError.mockReset();
  });

  it("resyncs Skills to every app on demand", async () => {
    m.resync.mockResolvedValue([
      { app: "claude", ok: true, failedSkills: [] },
      { app: "codex", ok: true, failedSkills: [] },
    ]);
    renderSheet();

    expect(
      screen.getByText("skills.storageSheet.resyncHint"),
    ).toBeInTheDocument();
    await userEvent.click(
      screen.getByRole("button", { name: "skills.storageSheet.resync" }),
    );

    await waitFor(() => expect(m.resync).toHaveBeenCalledTimes(1));
    expect(m.toastSuccess).toHaveBeenCalledWith(
      "skills.storageSheet.resyncDone",
      expect.anything(),
    );
  });

  it("names the apps and Skills that did not sync", async () => {
    m.resync.mockResolvedValue([
      { app: "claude", ok: true, failedSkills: [] },
      {
        app: "codex",
        ok: false,
        failedSkills: [{ directory: "pdf", error: "symlink failed" }],
      },
      {
        app: "hermes",
        ok: false,
        error: "permission denied",
        failedSkills: [],
      },
    ]);
    renderSheet();

    await userEvent.click(
      screen.getByRole("button", { name: "skills.storageSheet.resync" }),
    );

    await waitFor(() => expect(m.toastWarning).toHaveBeenCalledTimes(1));
    const [title, options] = m.toastWarning.mock.calls[0];
    expect(title).toBe("skills.storageSheet.resyncPartial");
    expect(options.description).toContain("Codex");
    expect(options.description).toContain("Hermes: permission denied");
    expect(m.toastSuccess).not.toHaveBeenCalled();
  });
});
