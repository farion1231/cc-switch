import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import i18n from "i18next";
import zh from "@/i18n/locales/zh.json";
import { DesktopSessionMigrationDialog } from "@/components/settings/DesktopSessionMigrationDialog";
import {
  desktopMigrationApi,
  profileId,
  type Inventory,
  type ProfileKey,
} from "@/lib/api/desktopMigration";

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
  profiles: [
    {
      key: source,
      lastKnownAccount: false,
      warnings: [],
      sessions: [
        {
          id: "local_active",
          title: "Active fixture",
          cwd: "/fictional",
          archived: false,
        },
        {
          id: "local_archived",
          title: "Archived fixture",
          cwd: "/fictional",
          archived: true,
        },
      ],
    },
    {
      key: target,
      lastKnownAccount: true,
      warnings: [],
      sessions: [],
    },
  ],
});

function setup() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  render(
    <QueryClientProvider client={client}>
      <DesktopSessionMigrationDialog open onOpenChange={vi.fn()} />
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
  vi.spyOn(desktopMigrationApi, "inventory").mockResolvedValue(fixtures());
  vi.spyOn(desktopMigrationApi, "preview").mockResolvedValue({
    token: "reviewed-snapshot",
    canApply: true,
    items: [{ id: "local_active", disposition: { kind: "ready" } }],
  });
  vi.spyOn(desktopMigrationApi, "apply").mockResolvedValue({
    copiedSessions: ["local_active"],
    skipped: [],
    error: null,
  });
});

describe("DesktopSessionMigrationDialog", () => {
  it("requires a preview, invalidates it after target changes, and applies its exact request and token", async () => {
    setup();
    await chooseProfiles();
    fireEvent.click(screen.getByRole("checkbox", { name: /Active fixture/ }));
    fireEvent.click(screen.getByRole("button", { name: "预览恢复" }));
    await screen.findByRole("button", { name: "执行恢复" });
    expect(desktopMigrationApi.apply).not.toHaveBeenCalled();
    fireEvent.change(
      screen.getByRole("combobox", { name: "目标账号 / 配置" }),
      { target: { value: "" } },
    );
    expect(
      screen.queryByRole("button", { name: "执行恢复" }),
    ).not.toBeInTheDocument();
    fireEvent.change(
      screen.getByRole("combobox", { name: "目标账号 / 配置" }),
      { target: { value: profileId(target) } },
    );
    fireEvent.click(screen.getByRole("button", { name: "预览恢复" }));
    fireEvent.click(await screen.findByRole("button", { name: "执行恢复" }));
    await waitFor(() =>
      expect(desktopMigrationApi.apply).toHaveBeenCalledWith(
        { source, target, sessionIds: ["local_active"] },
        "reviewed-snapshot",
      ),
    );
    expect(await screen.findByText(/已复制 1 条会话记录/)).toBeInTheDocument();
  });

  it("reports partial success and keeps the failure visible without retrying writes", async () => {
    vi.mocked(desktopMigrationApi.apply).mockResolvedValue({
      copiedSessions: ["local_active"],
      skipped: [],
      error: "Desktop started during recovery",
    });
    setup();
    await chooseProfiles();
    fireEvent.click(screen.getByRole("checkbox", { name: /Active fixture/ }));
    fireEvent.click(screen.getByRole("button", { name: "预览恢复" }));
    fireEvent.click(await screen.findByRole("button", { name: "执行恢复" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Desktop started during recovery",
    );
    expect(screen.getByText(/已复制 1 条会话记录/)).toBeInTheDocument();
    expect(desktopMigrationApi.apply).toHaveBeenCalledTimes(1);
  });
});
