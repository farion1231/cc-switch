import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi, beforeEach } from "vitest";
import type { McpServer, McpServersMap } from "@/types";

import UnifiedMcpPanel from "@/components/mcp/UnifiedMcpPanel";

const serversMock = vi.fn();

vi.mock("sonner", () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
    info: vi.fn(),
  },
}));

vi.mock("@/hooks/useMcp", () => ({
  useAllMcpServers: () => serversMock(),
  useToggleMcpApp: () => ({ mutateAsync: vi.fn() }),
  useDeleteMcpServer: () => ({ mutateAsync: vi.fn() }),
  useImportMcpFromApps: () => ({ mutateAsync: vi.fn() }),
}));

describe("UnifiedMcpPanel", () => {
  beforeEach(() => {
    serversMock.mockReturnValue({
      data: {},
      isLoading: false,
    });
  });

  it("filters MCP servers by the selected app and restores the original total view", async () => {
    const servers: McpServersMap = {
      "claude-mcp": createMcpServer("claude-mcp", { claude: true }),
      "codex-mcp": createMcpServer("codex-mcp", { codex: true }),
      "shared-mcp": createMcpServer("shared-mcp", {
        claude: true,
        codex: true,
      }),
    };
    serversMock.mockReturnValue({ data: servers, isLoading: false });

    render(<UnifiedMcpPanel onOpenChange={() => {}} />);

    await waitFor(() => {
      expect(screen.getByText("claude-mcp")).toBeInTheDocument();
      expect(screen.getByText("codex-mcp")).toBeInTheDocument();
      expect(screen.getByText("shared-mcp")).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole("button", { name: "Filter by Claude" }));

    await waitFor(() => {
      expect(screen.getByText("claude-mcp")).toBeInTheDocument();
      expect(screen.getByText("shared-mcp")).toBeInTheDocument();
      expect(screen.queryByText("codex-mcp")).not.toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole("button", { name: "Filter by Codex" }));

    await waitFor(() => {
      expect(screen.getByText("codex-mcp")).toBeInTheDocument();
      expect(screen.getByText("shared-mcp")).toBeInTheDocument();
      expect(screen.queryByText("claude-mcp")).not.toBeInTheDocument();
    });

    fireEvent.click(
      screen.getByRole("button", { name: "mcp.serverCount" }),
    );

    await waitFor(() => {
      expect(screen.getByText("claude-mcp")).toBeInTheDocument();
      expect(screen.getByText("codex-mcp")).toBeInTheDocument();
      expect(screen.getByText("shared-mcp")).toBeInTheDocument();
    });
  });
});

function createMcpServer(
  id: string,
  apps: Partial<McpServer["apps"]> = {},
): McpServer {
  return {
    id,
    name: id,
    server: { type: "stdio", command: "mock-server" },
    apps: {
      claude: false,
      codex: false,
      gemini: false,
      opencode: false,
      openclaw: false,
      hermes: false,
      ...apps,
    },
  };
}
