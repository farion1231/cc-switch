import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { http, HttpResponse } from "msw";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { EditProviderDialog } from "@/components/providers/EditProviderDialog";
import type { Provider } from "@/types";
import { server } from "../msw/server";

const apiMocks = vi.hoisted(() => ({
  getAll: vi.fn(),
  getCurrent: vi.fn(),
  getEditorView: vi.fn(),
  getLiveProviderSettings: vi.fn(),
  getOpenClawLiveProvider: vi.fn(),
}));

const toastMocks = vi.hoisted(() => ({
  success: vi.fn(),
  error: vi.fn(),
}));

vi.mock("@/lib/api", () => ({
  providersApi: {
    getAll: apiMocks.getAll,
    getCurrent: apiMocks.getCurrent,
    getEditorView: apiMocks.getEditorView,
  },
  vscodeApi: {
    getLiveProviderSettings: apiMocks.getLiveProviderSettings,
  },
  openclawApi: {
    getLiveProvider: apiMocks.getOpenClawLiveProvider,
  },
}));

vi.mock("@/lib/toast", () => ({
  toast: {
    success: (...args: unknown[]) => toastMocks.success(...args),
    error: (...args: unknown[]) => toastMocks.error(...args),
  },
}));

vi.mock("@/components/common/FullScreenPanel", () => ({
  FullScreenPanel: ({
    isOpen,
    actions,
    sidePanel,
    children,
    footer,
  }: {
    isOpen: boolean;
    actions?: React.ReactNode;
    sidePanel?: React.ReactNode;
    children: React.ReactNode;
    footer?: React.ReactNode;
  }) =>
    isOpen ? (
      <div>
        <div data-testid="panel-actions">{actions}</div>
        <div data-testid="panel-split">
          <div data-testid="panel-editor-pane">{children}</div>
          {sidePanel}
        </div>
        <div>{footer}</div>
      </div>
    ) : null,
}));

vi.mock("@/components/providers/forms/ProviderForm", () => ({
  ProviderForm: ({
    initialData,
  }: {
    initialData: {
      name?: string;
      settingsConfig?: Record<string, unknown>;
    };
  }) => (
    <div data-testid="provider-form-preview">
      <span data-testid="form-provider-name">{initialData.name}</span>
      <span data-testid="form-provider-config">
        {JSON.stringify(initialData.settingsConfig ?? {})}
      </span>
    </div>
  ),
}));

const TAURI_ENDPOINT = "http://tauri.local";

function makeProvider(
  name = "Codex Main",
  configText = 'model = "gpt-5.4"',
): Provider {
  return {
    id: "codex-1",
    name,
    settingsConfig: {
      auth: { OPENAI_API_KEY: "sk-test" },
      config: configText,
    },
    createdAt: 1700000000000,
  };
}

describe("EditProviderDialog right-side config history and rollback", () => {
  beforeEach(() => {
    toastMocks.success.mockReset();
    toastMocks.error.mockReset();
    apiMocks.getAll.mockReset();
    apiMocks.getCurrent.mockReset();
    apiMocks.getEditorView.mockReset();
    apiMocks.getLiveProviderSettings.mockReset();
    apiMocks.getOpenClawLiveProvider.mockReset();

    apiMocks.getCurrent.mockResolvedValue("codex-1");
    apiMocks.getLiveProviderSettings.mockResolvedValue({});
  });

  it("opens history from header button without count badge or redundant banner, previews with cache, and rolls back", async () => {
    const initialProvider = makeProvider("Codex Main", 'model = "gpt-5.4"');
    const rolledBackProvider = makeProvider("Codex Main", 'model = "gpt-5"');
    let restored = false;
    let previewCalls = 0;
    const restoreCalls: Array<{ app: string; backupId: string }> = [];

    apiMocks.getAll.mockImplementation(async () => ({
      "codex-1": restored ? rolledBackProvider : initialProvider,
    }));
    apiMocks.getEditorView.mockImplementation(async () => ({
      settings: (restored ? rolledBackProvider : initialProvider)
        .settingsConfig,
      isCurrent: true,
    }));

    const snapshotEntry = {
      id: "20261117_060000_0_import",
      app: "codex",
      reason: "import",
      createdAt: "2026-11-17T06:00:00Z",
      pinned: true,
      label: "import_default_config",
      totalSizeBytes: 512,
      files: [
        {
          key: "config.toml",
          originalPath: "/home/mock/.codex/config.toml",
          existed: true,
          sizeBytes: 512,
        },
      ],
    };

    server.use(
      http.post(`${TAURI_ENDPOINT}/list_app_config_backups`, () =>
        HttpResponse.json([snapshotEntry]),
      ),
      http.post(`${TAURI_ENDPOINT}/preview_app_config_backup`, () => {
        previewCalls += 1;
        return HttpResponse.json({
          entry: snapshotEntry,
          files: [
            {
              key: "config.toml",
              originalPath: "/home/mock/.codex/config.toml",
              existed: true,
              sizeBytes: 512,
              content: 'model = "gpt-5"\n',
              truncated: false,
              binary: false,
            },
            {
              key: "cc-switch-model-catalog.json",
              originalPath: "/home/mock/.codex/cc-switch-model-catalog.json",
              existed: true,
              sizeBytes: 32768,
              content: '{"models":[]}',
              truncated: false,
              binary: false,
            },
          ],
        });
      }),
      http.post(
        `${TAURI_ENDPOINT}/restore_app_config_backup`,
        async ({ request }) => {
          const body = (await request.json()) as {
            app: string;
            backupId: string;
          };
          restoreCalls.push(body);
          restored = true;
          return HttpResponse.json({
            restoredBackupId: body.backupId,
            safetyBackupId: null,
            restoredFiles: ["/home/mock/.codex/config.toml"],
          });
        },
      ),
    );

    render(
      <EditProviderDialog
        open
        onOpenChange={vi.fn()}
        onSubmit={vi.fn()}
        provider={initialProvider}
        appId="codex"
      />,
    );

    await waitFor(() => {
      expect(screen.getByTestId("form-provider-config").textContent).toContain(
        "gpt-5.4",
      );
    });

    // Header button has no count badge, and no redundant banner exists above the form
    const historyBtn = screen.getByRole("button", {
      name: "provider.configHistory.button",
    });
    expect(historyBtn.textContent).toBe("provider.configHistory.button");
    expect(
      screen.queryByRole("button", {
        name: "provider.configHistory.openAction",
      }),
    ).not.toBeInTheDocument();

    // Open right-side history panel from header button
    fireEvent.click(historyBtn);

    await waitFor(() => {
      expect(
        screen.getByRole("complementary", {
          name: "provider.configHistory.panelTitle",
        }),
      ).toBeInTheDocument();
      expect(screen.getByText(/config\.toml/)).toBeInTheDocument();
    });
    expect(screen.getByTestId("panel-editor-pane")).toBeVisible();

    // Preview inline: primary config.toml expanded immediately, large catalog collapsed by default
    fireEvent.click(
      screen.getByRole("button", { name: "provider.configHistory.preview" }),
    );
    await waitFor(() => {
      expect(screen.getByText(/model = "gpt-5"/)).toBeInTheDocument();
    });
    expect(screen.queryByText('{"models":[]}')).not.toBeInTheDocument();

    // Going back and previewing again hits client cache instead of extra IPC call
    fireEvent.click(
      screen.getByRole("button", { name: "provider.configHistory.backToList" }),
    );
    fireEvent.click(
      screen.getByRole("button", { name: "provider.configHistory.preview" }),
    );
    await waitFor(() => {
      expect(screen.getByText(/model = "gpt-5"/)).toBeInTheDocument();
    });
    expect(previewCalls).toBe(1);

    // Inline rollback confirmation inside right preview view
    fireEvent.click(
      screen.getByRole("button", { name: "provider.configHistory.restore" }),
    );
    await waitFor(() => {
      expect(
        screen.getByText("provider.configHistory.confirmRestoreMessage"),
      ).toBeInTheDocument();
    });

    fireEvent.click(
      screen.getByRole("button", {
        name: "provider.configHistory.confirmRestoreAction",
      }),
    );

    await waitFor(() => {
      expect(restoreCalls).toEqual([
        { app: "codex", backupId: "20261117_060000_0_import" },
      ]);
      expect(apiMocks.getAll).toHaveBeenCalledWith("codex");
      expect(screen.getByTestId("form-provider-config").textContent).toContain(
        "gpt-5",
      );
    });
    expect(toastMocks.success).toHaveBeenCalled();
  });
});
