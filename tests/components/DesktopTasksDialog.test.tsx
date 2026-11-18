import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import i18n from "i18next";
import zh from "@/i18n/locales/zh.json";
import { DesktopTasksDialog } from "@/components/sessions/DesktopTasksDialog";
import {
  desktopTasksApi,
  profileId,
  type Inventory,
  type ProfileKey,
} from "@/lib/api/desktopTasks";

const source: ProfileKey = {
  root: "default",
  account: "source-account",
  org: "source-org",
};
const target: ProfileKey = {
  root: "managed",
  account: "target-account",
  org: "target-org",
};

const fixtures = (): Inventory => ({
  writesSupported: true,
  warnings: [],
  orphanPrompts: ["orphan-instructions"],
  profiles: [
    {
      key: source,
      lastKnownAccount: false,
      warnings: [],
      tasks: [
        {
          id: "enabled-fixture",
          title: "Enabled fixture",
          enabled: true,
          schedule: { kind: "cron", expression: "0 9 * * *" },
          cwd: "/fictional",
          promptAvailable: true,
          problem: null,
        },
        {
          id: "paused-fixture",
          title: "Paused fixture",
          enabled: false,
          schedule: { kind: "manual" },
          cwd: "/fictional",
          promptAvailable: true,
          problem: null,
        },
        {
          id: "expired-fixture",
          title: "Expired fixture",
          enabled: true,
          schedule: { kind: "once", at: 1 },
          cwd: "/fictional",
          promptAvailable: true,
          problem: "expired",
        },
      ],
    },
    {
      key: target,
      lastKnownAccount: true,
      warnings: [],
      tasks: [],
    },
  ],
});

function setup() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  render(
    <QueryClientProvider client={client}>
      <DesktopTasksDialog open onOpenChange={vi.fn()} />
    </QueryClientProvider>,
  );
}

async function chooseProfiles() {
  fireEvent.change(
    await screen.findByRole("combobox", { name: "来源账号 / 配置" }),
    { target: { value: profileId(source) } },
  );
  fireEvent.change(screen.getByRole("combobox", { name: "目标账号 / 配置" }), {
    target: { value: profileId(target) },
  });
}

beforeEach(() => {
  vi.restoreAllMocks();
  i18n.addResourceBundle("zh", "translation", zh, true, true);
  vi.spyOn(desktopTasksApi, "inventory").mockResolvedValue(fixtures());
  vi.spyOn(desktopTasksApi, "preview").mockResolvedValue({
    token: "reviewed-snapshot",
    canApply: true,
    items: [{ id: "enabled-fixture", disposition: { kind: "ready" } }],
  });
  vi.spyOn(desktopTasksApi, "apply").mockResolvedValue({
    restoredTasks: [],
    skipped: [],
    backupPath: null,
    error: null,
  });
  vi.spyOn(desktopTasksApi, "prompt").mockResolvedValue(
    "Fictional instructions, not an active schedule.",
  );
});

describe("DesktopTasksDialog", () => {
  it("keeps paused tasks selectable, blocks expired one-time tasks, and does not invent orphan schedules", async () => {
    setup();
    await chooseProfiles();
    expect(
      screen.getByRole("checkbox", { name: /Expired fixture/ }),
    ).toBeDisabled();
    fireEvent.click(screen.getByRole("checkbox", { name: /Paused fixture/ }));
    fireEvent.click(screen.getByRole("checkbox", { name: /Enabled fixture/ }));
    fireEvent.click(screen.getByRole("button", { name: "预览恢复" }));
    await waitFor(() =>
      expect(desktopTasksApi.preview).toHaveBeenCalledWith({
        source,
        target,
        taskIds: ["paused-fixture", "enabled-fixture"],
      }),
    );
    fireEvent.click(
      screen.getByRole("button", { name: "orphan-instructions" }),
    );
    expect(
      await screen.findByText(
        "Fictional instructions, not an active schedule.",
      ),
    ).toBeInTheDocument();
    expect(desktopTasksApi.apply).not.toHaveBeenCalled();
  });
  it("applies the reviewed task selection and displays partial failure without retrying", async () => {
    vi.mocked(desktopTasksApi.apply).mockResolvedValue({
      restoredTasks: ["enabled-fixture"],
      skipped: [],
      backupPath: "/fictional/backup.json",
      error: "Registry read-back failed",
    });
    setup();
    await chooseProfiles();
    fireEvent.click(screen.getByRole("checkbox", { name: /Enabled fixture/ }));
    fireEvent.click(screen.getByRole("button", { name: "预览恢复" }));
    fireEvent.click(await screen.findByRole("button", { name: "执行恢复" }));
    await waitFor(() =>
      expect(desktopTasksApi.apply).toHaveBeenCalledWith(
        { source, target, taskIds: ["enabled-fixture"] },
        "reviewed-snapshot",
      ),
    );
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Registry read-back failed",
    );
    expect(screen.getByText(/已恢复 1 个任务/)).toBeInTheDocument();
    expect(desktopTasksApi.apply).toHaveBeenCalledTimes(1);
  });
});
