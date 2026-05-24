import { createRef } from "react";
import { act, render, waitFor } from "@testing-library/react";
import { describe, expect, it, vi, beforeEach } from "vitest";

import UnifiedMcpPanel, {
  type UnifiedMcpPanelHandle,
} from "@/components/mcp/UnifiedMcpPanel";

const importMcpMock = vi.fn();

vi.mock("sonner", () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
  },
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, params?: Record<string, unknown>) =>
      params ? `${key}:${JSON.stringify(params)}` : key,
  }),
  initReactI18next: { type: "3rdParty", init: () => {} },
}));

vi.mock("@/hooks/useMcp", () => ({
  useAllMcpServers: () => ({
    data: {},
    isLoading: false,
  }),
  useToggleMcpApp: () => ({
    mutateAsync: vi.fn(),
  }),
  useDeleteMcpServer: () => ({
    mutateAsync: vi.fn(),
  }),
  useImportMcpFromApps: () => ({
    mutateAsync: (...args: unknown[]) => importMcpMock(...args),
    isPending: false,
  }),
}));

vi.mock("@/components/ui/button", () => ({
  Button: ({ children, onClick, type = "button", ...rest }: any) => (
    <button type={type} onClick={onClick} {...rest}>
      {children}
    </button>
  ),
}));

vi.mock("@/components/ui/badge", () => ({
  Badge: ({ children, ...rest }: any) => <span {...rest}>{children}</span>,
}));

vi.mock("@/components/ui/tooltip", () => ({
  TooltipProvider: ({ children }: any) => <div>{children}</div>,
  Tooltip: ({ children }: any) => <div>{children}</div>,
  TooltipTrigger: ({ children }: any) => <>{children}</>,
  TooltipContent: ({ children }: any) => <div>{children}</div>,
}));

vi.mock("@/components/mcp/McpFormModal", () => ({
  default: () => <div data-testid="mcp-form" />,
}));

vi.mock("@/components/ConfirmDialog", () => ({
  ConfirmDialog: () => null,
}));

describe("UnifiedMcpPanel", () => {
  beforeEach(() => {
    importMcpMock.mockReset();
    importMcpMock.mockResolvedValue(0);
  });

  it("imports MCP servers only from visible apps", async () => {
    const ref = createRef<UnifiedMcpPanelHandle>();

    render(
      <UnifiedMcpPanel
        ref={ref}
        onOpenChange={() => {}}
        visibleApps={{
          claude: true,
          "claude-desktop": false,
          codex: true,
          gemini: false,
          opencode: true,
          openclaw: true,
          hermes: false,
        }}
      />,
    );

    await act(async () => {
      await ref.current?.openImport();
    });

    await waitFor(() => expect(importMcpMock).toHaveBeenCalledTimes(1));
    expect(importMcpMock).toHaveBeenCalledWith(["claude", "codex", "opencode"]);
  });
});
