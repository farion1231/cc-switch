import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { SkillsStorageSheet } from "@/components/skills/SkillsStorageSheet";

const m = vi.hoisted(() => ({
  settings: {
    skillStorageLocation: "cc_switch",
    skillSyncMethod: "auto",
  } as Record<string, string | undefined>,
  installed: [] as { id: string }[],
  updateSettings: vi.fn(),
  autoSaveSettings: vi.fn(),
  resync: vi.fn(),
  migrate: vi.fn(),
  openDir: vi.fn(),
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
    settings: m.settings,
    updateSettings: m.updateSettings,
    autoSaveSettings: m.autoSaveSettings,
  }),
}));

vi.mock("@/hooks/useSkills", () => ({
  useInstalledSkills: () => ({ data: m.installed }),
  useCcSwitchSkillsDir: () => ({ data: "/Users/jason/.cc-switch/skills" }),
  useResyncSkillsToApps: () => ({ mutateAsync: m.resync, isPending: false }),
}));

vi.mock("@/lib/api/skills", () => ({
  skillsApi: {
    migrateStorage: m.migrate,
    openCcSwitchSkillsDir: m.openDir,
  },
}));

const renderDialog = () =>
  render(<SkillsStorageSheet open onOpenChange={vi.fn()} />);

const radio = (name: string | RegExp) => screen.getByRole("radio", { name });

const moveButton = () =>
  screen.getByRole("button", { name: /skills\.storageSheet\.move$/ });

describe("SkillsStorageSheet", () => {
  beforeEach(() => {
    m.settings = { skillStorageLocation: "cc_switch", skillSyncMethod: "auto" };
    m.installed = [];
    m.autoSaveSettings.mockResolvedValue(undefined);
  });

  it.each(["auto", "symlink", "copy"] as const)(
    "shows the backend sync method %s as the checked option",
    (method) => {
      m.settings = { ...m.settings, skillSyncMethod: method };
      renderDialog();

      const options = {
        auto: radio("skills.storageSheet.syncAuto"),
        symlink: radio(/skills\.storageSheet\.syncSymlink/),
        copy: radio(/skills\.storageSheet\.syncCopy/),
      };
      for (const [id, element] of Object.entries(options)) {
        if (id === method) expect(element).toBeChecked();
        else expect(element).not.toBeChecked();
      }
    },
  );

  it("treats a missing sync method as automatic, like the backend default", () => {
    m.settings = { skillStorageLocation: "cc_switch" };
    renderDialog();

    expect(radio("skills.storageSheet.syncAuto")).toBeChecked();
  });

  it("saves the picked sync method", async () => {
    renderDialog();

    await userEvent.click(radio(/skills\.storageSheet\.syncCopy/));

    expect(m.updateSettings).toHaveBeenCalledWith({ skillSyncMethod: "copy" });
    expect(m.autoSaveSettings).toHaveBeenCalledWith({
      skillSyncMethod: "copy",
    });
  });

  it("shows the real CC Switch folder and opens it", async () => {
    m.openDir.mockResolvedValue(undefined);
    renderDialog();

    expect(screen.getByText("~/.cc-switch/skills")).toBeInTheDocument();
    await userEvent.click(
      screen.getByRole("button", {
        name: "skills.storageSheet.openFolderAria",
      }),
    );

    expect(m.openDir).toHaveBeenCalledTimes(1);
  });

  it("keeps move-and-switch unavailable until another location is picked", async () => {
    m.installed = [{ id: "a" }, { id: "b" }];
    renderDialog();

    expect(radio("skills.storageSheet.locationCcSwitch")).toBeChecked();
    expect(moveButton()).toBeDisabled();
    // 原因是看得见的一行，按钮用 aria-describedby 指向它
    expect(moveButton()).toHaveAccessibleDescription(
      "skills.storageSheet.moveHint",
    );

    await userEvent.click(moveButton());
    expect(
      screen.queryByText("skills.storageSheet.moveConfirmTitle"),
    ).not.toBeInTheDocument();
    expect(m.migrate).not.toHaveBeenCalled();
  });

  it("confirms before moving Skills to the other location", async () => {
    m.installed = [{ id: "a" }, { id: "b" }];
    m.migrate.mockResolvedValue({
      migratedCount: 2,
      skippedCount: 0,
      errors: [],
    });
    renderDialog();

    await userEvent.click(radio(/skills\.storageSheet\.locationUnified/));
    expect(moveButton()).toBeEnabled();
    await userEvent.click(moveButton());

    expect(
      screen.getByText("skills.storageSheet.moveConfirmTitle"),
    ).toBeInTheDocument();
    expect(m.migrate).not.toHaveBeenCalled();

    await userEvent.click(
      screen.getByRole("button", {
        name: "skills.storageSheet.moveConfirmButton",
      }),
    );

    await waitFor(() => expect(m.migrate).toHaveBeenCalledWith("unified"));
    await waitFor(() =>
      expect(m.updateSettings).toHaveBeenCalledWith({
        skillStorageLocation: "unified",
      }),
    );
    expect(m.toastSuccess).toHaveBeenCalledWith(
      "skills.storageSheet.moveDone",
      expect.anything(),
    );
  });

  it("resyncs Skills to every app on demand", async () => {
    m.resync.mockResolvedValue([
      { app: "claude", ok: true, failedSkills: [] },
      { app: "codex", ok: true, failedSkills: [] },
    ]);
    renderDialog();

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
    renderDialog();

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

  it("closes from the solid Done button", async () => {
    const onOpenChange = vi.fn();
    render(<SkillsStorageSheet open onOpenChange={onOpenChange} />);

    await userEvent.click(screen.getByRole("button", { name: "common.done" }));

    expect(onOpenChange).toHaveBeenCalledWith(false);
  });
});
