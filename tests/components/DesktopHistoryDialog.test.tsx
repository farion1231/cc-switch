import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import i18n from "i18next";
import zh from "@/i18n/locales/zh.json";
import { DesktopHistoryDialog } from "@/components/sessions/DesktopHistoryDialog";
import {
  desktopHistoryApi,
  profileId,
  type DesktopInventory,
  type DesktopProfileKey,
} from "@/lib/api/desktopHistory";

const source: DesktopProfileKey = {
  root: "default",
  account: "source-account",
  org: "source-org",
};
const target: DesktopProfileKey = {
  root: "managed",
  account: "target-account",
  org: "target-org",
};

const fixtures = (): DesktopInventory => ({
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
          updatedAt: 1,
          transcripts: [
            {
              kind: "available",
              id: "cli-fixture",
              path: "/fictional/projects/cli-fixture.jsonl",
            },
          ],
        },
        {
          id: "local_archived",
          title: "Archived fixture",
          cwd: "/fictional",
          archived: true,
          updatedAt: 1,
          transcripts: [{ kind: "missing", id: "cli-missing" }],
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
  const onOpenTranscript = vi.fn();
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  render(
    <QueryClientProvider client={client}>
      <DesktopHistoryDialog
        open
        onOpenChange={vi.fn()}
        onOpenTranscript={onOpenTranscript}
      />
    </QueryClientProvider>,
  );
  return { onOpenTranscript };
}

async function chooseProfiles() {
  fireEvent.change(
    await screen.findByRole("combobox", { name: zh.desktopHistory.profile }),
    { target: { value: profileId(source) } },
  );
}

beforeEach(() => {
  vi.restoreAllMocks();
  i18n.addResourceBundle("zh", "translation", zh, true, true);
  vi.spyOn(desktopHistoryApi, "inventory").mockResolvedValue(fixtures());
});

describe("DesktopHistoryDialog", () => {
  it("shows archives only on request and distinguishes missing transcripts", async () => {
    setup();
    await chooseProfiles();
    expect(screen.getByText("Active fixture")).toBeInTheDocument();
    expect(screen.queryByText("Archived fixture")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("checkbox", { name: "包含已归档历史" }));
    expect(screen.getByText("Archived fixture")).toBeInTheDocument();
    expect(screen.getByText("对话 1 缺失")).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "执行恢复" }),
    ).not.toBeInTheDocument();
  });

  it("resolves transcript links through the owning Desktop profile before opening the reader", async () => {
    const session = {
      providerId: "claude",
      sessionId: "cli-fixture",
      sourcePath: "/fictional/projects/cli-fixture.jsonl",
    };
    vi.spyOn(desktopHistoryApi, "openTranscript").mockResolvedValue(session);
    const { onOpenTranscript } = setup();
    await chooseProfiles();
    fireEvent.click(screen.getByRole("button", { name: "阅读对话 1" }));
    await waitFor(() => expect(onOpenTranscript).toHaveBeenCalledWith(session));
    expect(desktopHistoryApi.openTranscript).toHaveBeenCalledWith(
      source,
      "local_active",
      "cli-fixture",
    );
  });
});
