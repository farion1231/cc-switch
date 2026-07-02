import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { CopyToAppsDialog } from "@/components/providers/CopyToAppsDialog";
import type { Provider } from "@/types";

// Mock i18next
vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: any) => {
      const translations: Record<string, string> = {
        "provider.copyToApps.title": "复制到其他应用",
        "provider.copyToApps.description": "选择要复制到的目标应用",
        "provider.copyToApps.selectAll": "全选",
        "provider.copyToApps.deselectAll": "取消全选",
        "provider.copyToApps.selectedCount": `已选择 ${options?.count || 0} 个应用`,
        "provider.copyToApps.copyButton": `复制到 ${options?.count || 0} 个应用`,
        "provider.copyToApps.copy": `复制到 ${options?.count || 0} 个应用`,
        "provider.copyToApps.copying": "复制中...",
        "common.cancel": "取消",
        "apps.claude": "Claude Code",
        "apps.codex": "Codex",
        "apps.gemini": "Gemini CLI",
        "apps.opencode": "OpenCode",
        "apps.openclaw": "OpenClaw",
        "apps.hermes": "Hermes",
        "apps.claudeDesktop": "Claude Desktop",
      };
      return translations[key] || key;
    },
  }),
}));

describe("CopyToAppsDialog", () => {
  const mockProvider: Provider = {
    id: "test-provider",
    name: "Test Provider",
    settingsConfig: {},
    category: "custom",
    createdAt: Date.now(),
    inFailoverQueue: false,
  };

  const mockOnClose = vi.fn();
  const mockOnCopy = vi.fn();

  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("renders dialog when open", () => {
    render(
      <CopyToAppsDialog
        isOpen={true}
        onClose={mockOnClose}
        provider={mockProvider}
        sourceApp="claude"
        onCopy={mockOnCopy}
      />,
    );

    expect(screen.getByText("复制到其他应用")).toBeInTheDocument();
  });

  it("does not render when closed", () => {
    const { container } = render(
      <CopyToAppsDialog
        isOpen={false}
        onClose={mockOnClose}
        provider={mockProvider}
        sourceApp="claude"
        onCopy={mockOnCopy}
      />,
    );

    expect(container.firstChild).toBeNull();
  });

  it("shows available target apps excluding source app", () => {
    render(
      <CopyToAppsDialog
        isOpen={true}
        onClose={mockOnClose}
        provider={mockProvider}
        sourceApp="claude"
        onCopy={mockOnCopy}
      />,
    );

    // Should show Codex and Gemini but not Claude
    expect(screen.queryByText(/Codex/i)).toBeInTheDocument();
    expect(screen.queryByText(/Gemini/i)).toBeInTheDocument();
  });

  it("allows selecting and deselecting apps", () => {
    render(
      <CopyToAppsDialog
        isOpen={true}
        onClose={mockOnClose}
        provider={mockProvider}
        sourceApp="claude"
        onCopy={mockOnCopy}
      />,
    );

    const codexCheckbox = screen.getByRole("checkbox", { name: /Codex/i });

    // Initially unchecked
    expect(codexCheckbox).not.toBeChecked();

    // Click to select
    fireEvent.click(codexCheckbox);
    expect(codexCheckbox).toBeChecked();

    // Click again to deselect
    fireEvent.click(codexCheckbox);
    expect(codexCheckbox).not.toBeChecked();
  });

  it("handles select all functionality", () => {
    render(
      <CopyToAppsDialog
        isOpen={true}
        onClose={mockOnClose}
        provider={mockProvider}
        sourceApp="claude"
        onCopy={mockOnCopy}
      />,
    );

    const selectAllCheckbox = screen.getByRole("checkbox", { name: /全选/i });
    const codexCheckbox = screen.getByRole("checkbox", { name: /Codex/i });
    const geminiCheckbox = screen.getByRole("checkbox", { name: /Gemini/i });

    // Click select all
    fireEvent.click(selectAllCheckbox);
    expect(codexCheckbox).toBeChecked();
    expect(geminiCheckbox).toBeChecked();

    // Click again to deselect all
    fireEvent.click(selectAllCheckbox);
    expect(codexCheckbox).not.toBeChecked();
    expect(geminiCheckbox).not.toBeChecked();
  });

  it("disables copy button when no apps selected", () => {
    render(
      <CopyToAppsDialog
        isOpen={true}
        onClose={mockOnClose}
        provider={mockProvider}
        sourceApp="claude"
        onCopy={mockOnCopy}
      />,
    );

    const copyButton = screen.getByRole("button", { name: /复制到 0 个应用/ });
    expect(copyButton).toBeDisabled();
  });

  it("enables copy button when apps are selected", () => {
    render(
      <CopyToAppsDialog
        isOpen={true}
        onClose={mockOnClose}
        provider={mockProvider}
        sourceApp="claude"
        onCopy={mockOnCopy}
      />,
    );

    const codexCheckbox = screen.getByRole("checkbox", { name: /Codex/i });
    fireEvent.click(codexCheckbox);

    const copyButton = screen.getByRole("button", { name: /复制到 1 个应用/ });
    expect(copyButton).not.toBeDisabled();
  });

  it("calls onCopy with selected apps when copy button clicked", async () => {
    mockOnCopy.mockResolvedValue(undefined);

    render(
      <CopyToAppsDialog
        isOpen={true}
        onClose={mockOnClose}
        provider={mockProvider}
        sourceApp="claude"
        onCopy={mockOnCopy}
      />,
    );

    const codexCheckbox = screen.getByRole("checkbox", { name: /Codex/i });
    const geminiCheckbox = screen.getByRole("checkbox", { name: /Gemini/i });

    fireEvent.click(codexCheckbox);
    fireEvent.click(geminiCheckbox);

    const copyButton = screen.getByRole("button", { name: /复制到 2 个应用/ });
    fireEvent.click(copyButton);

    await waitFor(() => {
      expect(mockOnCopy).toHaveBeenCalledWith(["codex", "gemini"]);
    });
  });

  it("calls onClose when cancel button clicked", () => {
    render(
      <CopyToAppsDialog
        isOpen={true}
        onClose={mockOnClose}
        provider={mockProvider}
        sourceApp="claude"
        onCopy={mockOnCopy}
      />,
    );

    const cancelButton = screen.getByRole("button", { name: /取消/i });
    fireEvent.click(cancelButton);

    expect(mockOnClose).toHaveBeenCalled();
  });

  it("closes dialog after successful copy", async () => {
    mockOnCopy.mockResolvedValue(undefined);

    render(
      <CopyToAppsDialog
        isOpen={true}
        onClose={mockOnClose}
        provider={mockProvider}
        sourceApp="claude"
        onCopy={mockOnCopy}
      />,
    );

    const codexCheckbox = screen.getByRole("checkbox", { name: /Codex/i });
    fireEvent.click(codexCheckbox);

    const copyButton = screen.getByRole("button", { name: /复制到 1 个应用/ });
    fireEvent.click(copyButton);

    await waitFor(() => {
      expect(mockOnClose).toHaveBeenCalled();
    });
  });

  it("keeps dialog open on copy error", async () => {
    mockOnCopy.mockRejectedValue(new Error("Copy failed"));

    render(
      <CopyToAppsDialog
        isOpen={true}
        onClose={mockOnClose}
        provider={mockProvider}
        sourceApp="claude"
        onCopy={mockOnCopy}
      />,
    );

    const codexCheckbox = screen.getByRole("checkbox", { name: /Codex/i });
    fireEvent.click(codexCheckbox);

    const copyButton = screen.getByRole("button", { name: /复制到 1 个应用/ });
    fireEvent.click(copyButton);

    await waitFor(() => {
      expect(mockOnCopy).toHaveBeenCalled();
    });

    // Dialog should remain open
    expect(mockOnClose).not.toHaveBeenCalled();
  });
});
