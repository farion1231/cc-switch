import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { ReactElement } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import UnifiedMcpPanel from "@/components/mcp/UnifiedMcpPanel";

const toastSuccessMock = vi.fn();
const toastErrorMock = vi.fn();
const toastInfoMock = vi.fn();
const toastWarningMock = vi.fn();

const useAllMcpServersMock = vi.fn();
const useToggleMcpAppMock = vi.fn();
const useDeleteMcpServerMock = vi.fn();
const useImportMcpFromAppsMock = vi.fn();

const toggleMcpMutateAsyncMock = vi.fn();
const deleteMcpMutateAsyncMock = vi.fn();
const toggleAppApiMock = vi.fn();
const deleteUnifiedServerApiMock = vi.fn();

vi.mock("sonner", () => ({
  toast: {
    success: (...args: unknown[]) => toastSuccessMock(...args),
    error: (...args: unknown[]) => toastErrorMock(...args),
    info: (...args: unknown[]) => toastInfoMock(...args),
    warning: (...args: unknown[]) => toastWarningMock(...args),
  },
}));

vi.mock("@/hooks/useMcp", () => ({
  useAllMcpServers: (...args: unknown[]) => useAllMcpServersMock(...args),
  useToggleMcpApp: (...args: unknown[]) => useToggleMcpAppMock(...args),
  useDeleteMcpServer: (...args: unknown[]) => useDeleteMcpServerMock(...args),
  useImportMcpFromApps: (...args: unknown[]) =>
    useImportMcpFromAppsMock(...args),
}));

vi.mock("@/lib/api", async () => {
  const actual = await vi.importActual<object>("@/lib/api");
  return {
    ...actual,
    settingsApi: {
      openExternal: vi.fn(),
    },
  };
});

vi.mock("@/lib/api/mcp", () => ({
  mcpApi: {
    toggleApp: (...args: unknown[]) => toggleAppApiMock(...args),
    deleteUnifiedServer: (...args: unknown[]) =>
      deleteUnifiedServerApiMock(...args),
  },
}));

vi.mock("@/components/ConfirmDialog", () => ({
  ConfirmDialog: ({ isOpen, title, message, onConfirm, onCancel }: any) =>
    isOpen ? (
      <div data-testid="confirm-dialog">
        <div>{title}</div>
        <div>{message}</div>
        <button onClick={() => onConfirm()}>confirm-action</button>
        <button onClick={() => onCancel()}>cancel-action</button>
      </div>
    ) : null,
}));

function renderWithQueryClient(ui: ReactElement) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });

  return render(
    <QueryClientProvider client={queryClient}>{ui}</QueryClientProvider>,
  );
}

const mcpServers = {
  "server-1": {
    id: "server-1",
    name: "Server One",
    server: { type: "stdio", command: "npx", args: ["server-one"] },
    apps: {
      claude: false,
      codex: false,
      gemini: false,
      opencode: false,
      openclaw: false,
    },
  },
  "server-2": {
    id: "server-2",
    name: "Server Two",
    server: { type: "stdio", command: "npx", args: ["server-two"] },
    apps: {
      claude: true,
      codex: false,
      gemini: false,
      opencode: false,
      openclaw: false,
    },
  },
};

describe("UnifiedMcpPanel", () => {
  beforeEach(() => {
    toastSuccessMock.mockReset();
    toastErrorMock.mockReset();
    toastInfoMock.mockReset();
    toastWarningMock.mockReset();
    toggleMcpMutateAsyncMock.mockReset();
    deleteMcpMutateAsyncMock.mockReset();
    toggleAppApiMock.mockReset();
    deleteUnifiedServerApiMock.mockReset();

    useAllMcpServersMock.mockReturnValue({
      data: mcpServers,
      isLoading: false,
    });
    useToggleMcpAppMock.mockReturnValue({
      mutateAsync: toggleMcpMutateAsyncMock,
    });
    useDeleteMcpServerMock.mockReturnValue({
      mutateAsync: deleteMcpMutateAsyncMock,
    });
    useImportMcpFromAppsMock.mockReturnValue({
      mutateAsync: vi.fn(),
    });
  });

  it("defaults to single mode and keeps apply changes disabled", () => {
    renderWithQueryClient(<UnifiedMcpPanel onOpenChange={vi.fn()} />);

    expect(screen.getByRole("switch")).not.toBeChecked();
    expect(screen.getByText("Apply changes (0)")).toBeDisabled();
  });

  it("applies single-app toggles immediately in single mode", async () => {
    toggleMcpMutateAsyncMock.mockResolvedValue(true);

    renderWithQueryClient(<UnifiedMcpPanel onOpenChange={vi.fn()} />);

    fireEvent.click(screen.getAllByRole("button", { name: "Claude" })[0]);

    await waitFor(() => {
      expect(toggleMcpMutateAsyncMock).toHaveBeenCalledWith({
        serverId: "server-1",
        app: "claude",
        enabled: true,
      });
    });
  });

  it("stages changes for all selected servers in batch mode and shows pending dots", () => {
    renderWithQueryClient(<UnifiedMcpPanel onOpenChange={vi.fn()} />);

    fireEvent.click(screen.getByRole("switch"));
    fireEvent.click(screen.getAllByRole("checkbox")[0]);
    fireEvent.click(screen.getAllByRole("checkbox")[1]);
    fireEvent.click(screen.getAllByRole("button", { name: "Gemini" })[0]);

    expect(toggleMcpMutateAsyncMock).not.toHaveBeenCalled();
    expect(screen.getByText("Apply changes (2)")).not.toBeDisabled();
    expect(screen.getAllByLabelText("Gemini pending")).toHaveLength(2);
  });

  it("auto-selects the clicked server when staging without a prior selection", () => {
    renderWithQueryClient(<UnifiedMcpPanel onOpenChange={vi.fn()} />);

    fireEvent.click(screen.getByRole("switch"));
    fireEvent.click(screen.getAllByRole("button", { name: "OpenCode" })[0]);

    expect(screen.getByText("Apply changes (1)")).not.toBeDisabled();
    expect(screen.getByLabelText("Select MCP server Server One")).toBeChecked();
    expect(screen.getAllByLabelText("OpenCode pending")).toHaveLength(1);
  });

  it("confirms and submits staged changes for all selected servers", async () => {
    toggleAppApiMock.mockResolvedValue(true);

    renderWithQueryClient(<UnifiedMcpPanel onOpenChange={vi.fn()} />);

    fireEvent.click(screen.getByRole("switch"));
    fireEvent.click(screen.getAllByRole("checkbox")[0]);
    fireEvent.click(screen.getAllByRole("checkbox")[1]);
    fireEvent.click(screen.getAllByRole("button", { name: "Codex" })[0]);
    fireEvent.click(screen.getByText("Apply changes (2)"));
    fireEvent.click(screen.getByText("confirm-action"));

    await waitFor(() => {
      expect(toggleAppApiMock).toHaveBeenCalledTimes(2);
      expect(toggleAppApiMock).toHaveBeenCalledWith("server-1", "codex", true);
      expect(toggleAppApiMock).toHaveBeenCalledWith("server-2", "codex", true);
    });
  });

  it("keeps failed pending changes after partial apply failure", async () => {
    let callCount = 0;
    toggleAppApiMock.mockImplementation(async () => {
      callCount += 1;
      if (callCount === 2) {
        throw new Error("apply failed");
      }
      return true;
    });

    renderWithQueryClient(<UnifiedMcpPanel onOpenChange={vi.fn()} />);

    fireEvent.click(screen.getByRole("switch"));
    fireEvent.click(screen.getAllByRole("checkbox")[0]);
    fireEvent.click(screen.getAllByRole("checkbox")[1]);
    fireEvent.click(screen.getAllByRole("button", { name: "Codex" })[0]);
    fireEvent.click(screen.getByText("Apply changes (2)"));
    fireEvent.click(screen.getByText("confirm-action"));

    await waitFor(() => {
      expect(toastWarningMock).toHaveBeenCalled();
    });

    expect(screen.getByText("Apply changes (1)")).not.toBeDisabled();
    expect(screen.getByLabelText("Codex pending")).toBeInTheDocument();
  });

  it("confirms and deletes all selected servers in batch mode", async () => {
    deleteUnifiedServerApiMock.mockResolvedValue(true);

    renderWithQueryClient(<UnifiedMcpPanel onOpenChange={vi.fn()} />);

    fireEvent.click(screen.getAllByRole("checkbox")[0]);
    fireEvent.click(screen.getAllByRole("checkbox")[1]);
    fireEvent.click(
      screen.getByRole("button", {
        name: /Delete Selected|删除已选|mcp\.bulkDelete/,
      }),
    );
    fireEvent.click(screen.getByText("confirm-action"));

    await waitFor(() => {
      expect(deleteUnifiedServerApiMock).toHaveBeenCalledTimes(2);
      expect(deleteUnifiedServerApiMock).toHaveBeenCalledWith("server-1");
      expect(deleteUnifiedServerApiMock).toHaveBeenCalledWith("server-2");
    });
  });
});
