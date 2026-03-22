import { createRef } from "react";
import { act, render } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import UnifiedMcpPanel, {
  type UnifiedMcpPanelHandle,
} from "@/components/mcp/UnifiedMcpPanel";
import { toast } from "sonner";

const importMock = vi.fn();
const toggleAppMock = vi.fn();
const deleteServerMock = vi.fn();

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, params?: Record<string, unknown>) =>
      params ? `${key}:${JSON.stringify(params)}` : key,
  }),
}));

vi.mock("sonner", () => ({
  toast: {
    success: vi.fn(),
    warning: vi.fn(),
    error: vi.fn(),
  },
}));

vi.mock("@/hooks/useMcp", () => ({
  useAllMcpServers: () => ({
    data: {},
    isLoading: false,
  }),
  useToggleMcpApp: () => ({
    mutateAsync: toggleAppMock,
  }),
  useDeleteMcpServer: () => ({
    mutateAsync: deleteServerMock,
  }),
  useImportMcpFromApps: () => ({
    mutateAsync: importMock,
  }),
}));

describe("UnifiedMcpPanel", () => {
  beforeEach(() => {
    importMock.mockReset();
    toggleAppMock.mockReset();
    deleteServerMock.mockReset();
    vi.mocked(toast.success).mockReset();
    vi.mocked(toast.warning).mockReset();
    vi.mocked(toast.error).mockReset();
  });

  it("shows no-import toast when nothing changed", async () => {
    importMock.mockResolvedValue({
      added: 0,
      refreshed: 0,
      enabledOnly: 0,
      conflicts: 0,
      invalid: 0,
      issues: [],
    });

    const ref = createRef<UnifiedMcpPanelHandle>();
    render(<UnifiedMcpPanel ref={ref} onOpenChange={() => {}} />);

    await act(async () => {
      await ref.current?.openImport();
    });

    expect(toast.success).toHaveBeenCalledWith(
      "mcp.unifiedPanel.noImportFound",
      { closeButton: true },
    );
    expect(toast.warning).not.toHaveBeenCalled();
  });

  it("shows structured success and warning toasts", async () => {
    importMock.mockResolvedValue({
      added: 1,
      refreshed: 2,
      enabledOnly: 3,
      conflicts: 1,
      invalid: 1,
      issues: [
        {
          id: "shared",
          sourceApp: "codex",
          kind: "conflict",
          message: "",
          existingApps: ["claude", "gemini"],
        },
        {
          id: "broken",
          sourceApp: "claude",
          kind: "invalid",
          message: "",
          existingApps: [],
        },
      ],
    });

    const ref = createRef<UnifiedMcpPanelHandle>();
    render(<UnifiedMcpPanel ref={ref} onOpenChange={() => {}} />);

    await act(async () => {
      await ref.current?.openImport();
    });

    expect(toast.success).toHaveBeenCalledWith(
      expect.stringContaining("mcp.unifiedPanel.importSummary"),
      { closeButton: true },
    );
    expect(toast.warning).toHaveBeenCalledTimes(1);

    const [, options] = vi.mocked(toast.warning).mock.calls[0];
    expect(options?.description).toContain("shared");
    expect(options?.description).toContain("broken");
    expect(options?.description).toContain(
      "mcp.unifiedPanel.importWarningHint",
    );
  });
});
