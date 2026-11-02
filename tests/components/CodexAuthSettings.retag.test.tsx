import {
  act,
  render,
  screen,
  fireEvent,
  waitFor,
} from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { CodexAuthSettings } from "@/components/settings/CodexAuthSettings";
import type { SettingsFormState } from "@/hooks/useSettings";

const templates: Record<string, string> = {
  "confirm.codexUnifyHistoryRetag.message":
    "{{threads}} rows / {{files}} files",
  "settings.codexUnifyHistoryRetagCompleted":
    "retagged {{threads}} / {{files}}",
};

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: Record<string, unknown>) => {
      const template = templates[key] ?? key;
      if (!options) return template;
      return template.replace(/\{\{\s*(\w+)\s*\}\}/g, (_match, name) =>
        options[name] === undefined ? "" : String(options[name]),
      );
    },
  }),
}));

const previewMock = vi.fn();
const retagMock = vi.fn();
const getCompressionMock = vi.fn();
const diskUsageMock = vi.fn();
const toastMock = vi.hoisted(() => ({
  success: vi.fn(),
  error: vi.fn(),
  info: vi.fn(),
}));

vi.mock("@/lib/toast", () => ({ toast: toastMock }));

vi.mock("@/lib/api", () => ({
  settingsApi: {
    codexForcesMultiAgentV2: vi.fn().mockResolvedValue(false),
    hasCodexUnifyHistoryBackup: vi.fn().mockResolvedValue(false),
    restoreCodexUnifiedHistory: vi.fn(),
    getCodexSessionCompression: () => getCompressionMock(),
    setCodexSessionCompression: vi.fn(),
    getCodexSessionsDiskUsage: () => diskUsageMock(),
    previewCodexUnifyHistoryRetag: () => previewMock(),
    retagCodexUnifyHistory: () => retagMock(),
  },
}));

const ACTION = "settings.codexUnifyHistoryRetagAction";
const CONFIRM_TITLE = "confirm.codexUnifyHistoryRetag.title";
const CONFIRM_BUTTON = "confirm.codexUnifyHistoryRetag.confirm";

async function renderWith(unify: boolean) {
  await act(async () => {
    render(
      <CodexAuthSettings
        settings={{ unifyCodexSessionHistory: unify } as SettingsFormState}
        onChange={vi.fn()}
      />,
    );
  });
}

describe("CodexAuthSettings unified-history retag", () => {
  beforeEach(() => {
    previewMock.mockReset();
    retagMock.mockReset();
    toastMock.success.mockReset();
    toastMock.error.mockReset();
    toastMock.info.mockReset();
    getCompressionMock.mockResolvedValue(false);
    diskUsageMock.mockResolvedValue(0);
  });

  it("does not render the retag action while unified history is off", async () => {
    await renderWith(false);

    expect(screen.queryByRole("button", { name: ACTION })).toBeNull();
    expect(previewMock).not.toHaveBeenCalled();
  });

  it("reports live_not_unified without opening the confirm dialog", async () => {
    previewMock.mockResolvedValue({
      pendingThreadRows: 0,
      pendingJsonlFiles: 0,
      retaggedStateRows: 0,
      retaggedJsonlFiles: 0,
      skippedReason: "live_not_unified",
    });
    await renderWith(true);

    fireEvent.click(screen.getByRole("button", { name: ACTION }));

    await waitFor(() =>
      expect(toastMock.info).toHaveBeenCalledWith(
        "settings.codexUnifyHistoryRetagLiveNotUnified",
      ),
    );
    expect(screen.queryByText(CONFIRM_TITLE)).toBeNull();
    expect(retagMock).not.toHaveBeenCalled();
  });

  it("shows the preview counts in the confirm dialog, then toasts the retagged counts", async () => {
    previewMock.mockResolvedValue({
      pendingThreadRows: 3,
      pendingJsonlFiles: 1,
      retaggedStateRows: 0,
      retaggedJsonlFiles: 0,
    });
    retagMock.mockResolvedValue({
      pendingThreadRows: 3,
      pendingJsonlFiles: 1,
      retaggedStateRows: 3,
      retaggedJsonlFiles: 1,
    });
    await renderWith(true);

    fireEvent.click(screen.getByRole("button", { name: ACTION }));

    expect(await screen.findByText("3 rows / 1 files")).toBeTruthy();
    expect(screen.getByText(CONFIRM_TITLE)).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: CONFIRM_BUTTON }));

    await waitFor(() =>
      expect(toastMock.success).toHaveBeenCalledWith("retagged 3 / 1"),
    );
    expect(retagMock).toHaveBeenCalledTimes(1);
    expect(previewMock).toHaveBeenCalledTimes(1);
  });

  it("tells the user when nothing is pending instead of opening the dialog", async () => {
    previewMock.mockResolvedValue({
      pendingThreadRows: 0,
      pendingJsonlFiles: 0,
      retaggedStateRows: 0,
      retaggedJsonlFiles: 0,
      skippedReason: "nothing_pending",
    });
    await renderWith(true);

    fireEvent.click(screen.getByRole("button", { name: ACTION }));

    await waitFor(() =>
      expect(toastMock.info).toHaveBeenCalledWith(
        "settings.codexUnifyHistoryRetagNothing",
      ),
    );
    expect(screen.queryByText(CONFIRM_TITLE)).toBeNull();
    expect(retagMock).not.toHaveBeenCalled();
  });
});
